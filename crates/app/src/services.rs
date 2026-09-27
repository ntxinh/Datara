//! Process-wide services: Tokio runtime, storage, secret store, and the
//! shared [`DatabaseService`]. Created once on startup, then shared with the
//! Slint UI as `Arc<AppServices>`; backend work is spawned onto `runtime`
//! from UI callbacks (see `ui.rs`), results return via `UiHandle`.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use datara_config::{AppConfig, AppPaths};
use datara_database::DatabaseService;
use datara_domain::{
    AuthenticationMode, ConnectionId, ConnectionProfile, Credentials, DomainError, EncryptionMode,
    Result as DomainResult, SecretReference, TableKind,
};
use datara_driver_mssql::{quote_ident, MssqlDriver};
use datara_secrets::SecretStore;
use datara_storage::{NewConnection, Storage};
use secrecy::SecretString;
use tokio::runtime::Runtime;

use crate::bridge::{AppEvent, UiHandle};
use crate::schema_tree::{FolderKind, NodeKind, TreeNode};
use crate::ConnForm;

/// Multi-thread runtime for all backend I/O. Slint owns the main thread.
pub fn spawn_runtime() -> DomainResult<Runtime> {
    tokio::runtime::Builder::new_multi_thread()
        .thread_name("datara-db")
        .enable_all()
        .build()
        .map_err(|e| datara_domain::DomainError::Driver {
            message: format!("cannot start tokio runtime: {e}"),
        })
}

/// The async backend, minus the runtime: everything `runtime.spawn`ed tasks
/// need. Passed to the Tokio world as `Arc<Backend>` (`Runtime` itself stays
/// in [`AppServices`] on the UI side).
pub struct Backend {
    pub storage: Arc<Storage>,
    pub secrets: Arc<SecretStore>,
    pub service: Arc<DatabaseService<MssqlDriver>>,
    /// Last connection that produced schema data — the fallback target for
    /// editor tabs that weren't opened from a tree node (Task 4.2).
    last_conn_id: std::sync::atomic::AtomicI64,
    /// In-flight queries by editor tab id. `JoinHandle` isn't cloneable, so
    /// the map holds an `AbortHandle`; the awaiting task owns the join.
    running: parking_lot::Mutex<HashMap<i32, RunningQuery>>,
    /// `[query] timeout_seconds` from config.
    query_timeout: Duration,
}

/// Cancellation bookkeeping for one running query. `abort` is None while
/// the slot is reserved but the query hasn't spawned yet (connect path).
struct RunningQuery {
    abort: Option<tokio::task::AbortHandle>,
    conn_id: ConnectionId,
}

/// Long-lived application state: runtime + backend + config.
pub struct AppServices {
    pub runtime: Runtime,
    pub backend: Arc<Backend>,
    pub config: AppConfig,
}

impl AppServices {
    /// Resolve XDG paths, load config, open SQLite, connect the Secret
    /// Service. Synchronous wrapper around the async init.
    pub fn init() -> anyhow::Result<Self> {
        let runtime = spawn_runtime()?;
        let paths = AppPaths::new()?;
        let config = AppConfig::load(&paths)?;
        let storage = Arc::new(runtime.block_on(Storage::open(&paths.app_db()))?);
        let secrets = Arc::new(runtime.block_on(SecretStore::connect())?);
        let service = Arc::new(DatabaseService::<MssqlDriver>::new(
            Arc::clone(&storage),
            Arc::clone(&secrets),
        ));
        let query_timeout = Duration::from_secs(config.query.timeout_seconds);
        let backend = Arc::new(Backend {
            storage,
            secrets,
            service,
            last_conn_id: std::sync::atomic::AtomicI64::new(-1),
            running: parking_lot::Mutex::new(HashMap::new()),
            query_timeout,
        });
        Ok(Self {
            runtime,
            backend,
            config,
        })
    }
}

/// `SELECT TOP {limit} * FROM [schema].[table]` for `open-table`. Sync and
/// pure — generated on the UI thread; the Phase 5 grid will run it.
pub fn preview_sql(schema: &str, table: &str, limit: u32) -> String {
    format!(
        "SELECT TOP {limit} * FROM {}.{}",
        quote_ident(schema),
        quote_ident(table)
    )
}

/// Status-bar text for a failed query: `Msg {n}, Line {l}: {msg}` when the
/// server reported number/line, else the error's `Display`.
fn query_error_text(e: &DomainError) -> String {
    match e {
        DomainError::Query {
            message,
            error_number,
            line,
            column,
        } => {
            let mut head = Vec::new();
            if let Some(n) = error_number {
                head.push(format!("Msg {n}"));
            }
            if let Some(l) = line {
                head.push(format!("Line {l}"));
            }
            if let Some(c) = column {
                head.push(format!("Col {c}"));
            }
            if head.is_empty() {
                format!("Query failed: {message}")
            } else {
                format!("{}: {message}", head.join(", "))
            }
        }
        _ => e.to_string(),
    }
}

fn encryption_mode(s: &str) -> EncryptionMode {
    match s {
        "disabled" => EncryptionMode::Disabled,
        "required" => EncryptionMode::Required,
        _ => EncryptionMode::Preferred,
    }
}

/// Profile fields that come straight from the dialog (no id/secret ref).
fn form_fields(form: &ConnForm) -> DomainResult<NewConnection> {
    let port = u16::try_from(form.port).map_err(|_| datara_domain::DomainError::Storage {
        message: format!("invalid port {}", form.port),
    })?;
    Ok(NewConnection {
        name: form.name.to_string(),
        host: form.host.to_string(),
        port,
        database: match form.database.as_str() {
            "" => None,
            s => Some(s.to_owned()),
        },
        username: form.username.to_string(),
        authentication: AuthenticationMode::SqlPassword,
        encryption: encryption_mode(&form.encryption),
        trust_server_certificate: form.trust_cert,
    })
}

impl Backend {
    /// Load all profiles and push them to the sidebar.
    pub async fn reload_connections(&self, ui: &UiHandle) {
        match self.storage.connections().list().await {
            Ok(list) => ui.dispatch(AppEvent::ConnectionsLoaded(list)),
            Err(e) => ui.dispatch(AppEvent::Status(format!("Load failed: {e}"))),
        }
    }

    /// Save flow: insert the profile row, store its password under the
    /// derived secret reference, reload the list. On secret-save failure the
    /// row is rolled back — a profile without its secret is worse than none.
    pub async fn save_connection(&self, form: ConnForm, ui: UiHandle) {
        match self.save_inner(form).await {
            Ok(()) => {
                ui.dispatch(AppEvent::Status("Connection saved".into()));
                self.reload_connections(&ui).await;
            }
            Err(e) => ui.dispatch(AppEvent::Status(format!("Save failed: {e}"))),
        }
    }

    async fn save_inner(&self, form: ConnForm) -> DomainResult<()> {
        let new = form_fields(&form)?;
        let password = SecretString::from(form.password.to_string());
        let repo = self.storage.connections();
        let id = repo.insert(new).await?;
        // The reference is derived from the id by the repo; read it back
        // instead of duplicating the format.
        let reference = repo.get(id).await?.secret_reference;
        if let Err(e) = self
            .secrets
            .save(&reference, &format!("Datara — {}", form.name), &password)
            .await
        {
            let _ = repo.delete(id).await;
            return Err(e);
        }
        Ok(())
    }

    /// Test button: attempt a real connection with the form's fields, report
    /// the outcome into the dialog's `test-result` property.
    pub async fn test_form(&self, form: ConnForm, ui: UiHandle) {
        let result = match form_fields(&form).and_then(|new| self.form_profile(&form, new)) {
            Ok((profile, creds)) => self
                .service
                .test_connection(&profile, &creds)
                .await
                .map_err(|e| e.to_string()),
            Err(e) => Err(e.to_string()),
        };
        ui.dispatch(AppEvent::ConnectTestResult(result));
    }

    /// Wrap the dialog fields in a throwaway `ConnectionProfile` (id/secret
    /// ref are placeholders — nothing is persisted for a test).
    fn form_profile(
        &self,
        form: &ConnForm,
        new: NewConnection,
    ) -> DomainResult<(ConnectionProfile, Credentials)> {
        let profile = ConnectionProfile {
            id: ConnectionId(0),
            name: new.name,
            host: new.host,
            port: new.port,
            database: new.database,
            username: new.username.clone(),
            authentication: new.authentication,
            encryption: new.encryption,
            trust_server_certificate: new.trust_server_certificate,
            secret_reference: SecretReference(String::new()),
        };
        let creds = Credentials {
            username: new.username,
            password: SecretString::from(form.password.to_string()),
        };
        Ok((profile, creds))
    }

    /// Connection the editor falls back to when the active tab carries
    /// none — set by the last tree action that used a session.
    pub fn last_conn_id(&self) -> Option<ConnectionId> {
        let v = self.last_conn_id.load(std::sync::atomic::Ordering::Relaxed);
        (v >= 0).then_some(ConnectionId(v))
    }

    pub fn set_last_conn_id(&self, id: ConnectionId) {
        self.last_conn_id
            .store(id.0, std::sync::atomic::Ordering::Relaxed);
    }

    /// True while any tab has a query in flight — drives the Stop button.
    pub fn any_running(&self) -> bool {
        !self.running.lock().is_empty()
    }

    /// `Command::ExecuteQuery`: run `sql` for editor `tab_id` and report the
    /// outcome back through the status/results area. Phase 5 swaps the
    /// row-count status for the real grid.
    ///
    /// One query per tab at a time: a second Execute on a running tab is
    /// rejected. The handle is awaited under the configured timeout; on
    /// elapse the task is aborted and the pooled session dropped (the TDS
    /// stream may be mid-response and is not reusable).
    pub async fn execute_sql(
        &self,
        tab_id: i32,
        conn_id: ConnectionId,
        database: Option<String>,
        sql: String,
        max_rows: usize,
        ui: UiHandle,
    ) {
        self.set_last_conn_id(conn_id);
        // Reserve the slot before `execute` — the profile fetch + connect
        // inside it can take seconds, and a second Ctrl+Enter (or a
        // premature Cancel) must see this tab as busy.
        match self.running.lock().entry(tab_id) {
            std::collections::hash_map::Entry::Occupied(_) => {
                ui.dispatch(AppEvent::Status("Query already running on this tab".into()));
                return;
            }
            std::collections::hash_map::Entry::Vacant(v) => {
                v.insert(RunningQuery {
                    abort: None,
                    conn_id,
                });
            }
        }
        let mut handle = match self.service.execute(conn_id, database, sql, max_rows).await {
            Ok(h) => h,
            Err(e) => {
                self.running.lock().remove(&tab_id);
                ui.dispatch(AppEvent::QueryError {
                    tab: tab_id,
                    message: query_error_text(&e),
                });
                return;
            }
        };
        let armed = {
            let mut running = self.running.lock();
            match running.get_mut(&tab_id) {
                Some(rq) => {
                    rq.abort = Some(handle.abort_handle());
                    true
                }
                None => false,
            }
        };
        if !armed {
            // Cancelled while connecting: kill it before it starts —
            // cancel_query already dispatched QueryCancelled. The session
            // `execute` just obtained was inserted into the pool *after*
            // cancel's disconnect ran, so evict it again: aborting
            // mid-query may leave its TDS stream desynced.
            handle.abort();
            let _ = handle.await; // deterministic task teardown
            self.service.disconnect(conn_id).await;
            return;
        }
        ui.dispatch(AppEvent::QueryStarted { tab: tab_id });
        let started = std::time::Instant::now();
        let outcome = match tokio::time::timeout(self.query_timeout, &mut handle).await {
            Ok(Ok(Ok(r))) => Some(AppEvent::QueryResult {
                tab: tab_id,
                result: r,
                elapsed_ms: started.elapsed().as_millis() as u64,
            }),
            Ok(Ok(Err(e))) => Some(AppEvent::QueryError {
                tab: tab_id,
                message: query_error_text(&e),
            }),
            Ok(Err(je)) if je.is_cancelled() => {
                // The cancel path already dispatched QueryCancelled.
                None
            }
            Ok(Err(je)) => Some(AppEvent::QueryError {
                tab: tab_id,
                message: format!("task failed: {je}"),
            }),
            Err(_elapsed) => {
                // Same fix as an explicit cancel: abort + drop the session —
                // the server is still processing this batch.
                handle.abort();
                self.service.disconnect(conn_id).await;
                Some(AppEvent::QueryError {
                    tab: tab_id,
                    message: format!("Query timed out after {}s", self.query_timeout.as_secs()),
                })
            }
        };
        // Remove the slot BEFORE the terminal event: `apply` consults
        // `any_running()` when the event lands — dispatching first would
        // leave the Stop button visible.
        self.running.lock().remove(&tab_id);
        if let Some(event) = outcome {
            ui.dispatch(event);
        }
    }

    /// `Bridge.cancel-query` / Stop button: abort the running query — the
    /// active tab's when it has one, else whichever query is in flight —
    /// then drop its pooled session (aborting leaves the TDS stream
    /// mid-response; the next execute reconnects).
    pub async fn cancel_query(&self, tab_id: i32, ui: UiHandle) {
        let entry = {
            let mut running = self.running.lock();
            running.remove_entry(&tab_id).or_else(|| {
                running
                    .keys()
                    .next()
                    .copied()
                    .and_then(|k| running.remove_entry(&k))
            })
        };
        let Some((tab, rq)) = entry else {
            return;
        };
        if let Some(abort) = rq.abort {
            abort.abort();
        }
        self.service.disconnect(rq.conn_id).await;
        ui.dispatch(AppEvent::QueryCancelled { tab });
    }

    /// `toggle-node` on an unexpanded row: fetch its children by kind and
    /// dispatch them back for `SchemaTree::replace_children`.
    pub async fn expand_node(&self, node: TreeNode, ui: UiHandle) {
        if let Some(id) = node.connection_id {
            self.set_last_conn_id(id);
        }
        let result = self
            .node_children(&node)
            .await
            .map_err(|e| format!("{}: {e}", node.label));
        ui.dispatch(AppEvent::TreeChildren {
            parent_id: node.id,
            result,
        });
    }

    /// Children for one expanded row. Lazy: nothing here runs until expand.
    async fn node_children(&self, node: &TreeNode) -> DomainResult<Vec<TreeNode>> {
        let conn_id = || {
            node.connection_id.ok_or_else(|| DomainError::Driver {
                message: "tree node missing connection_id".into(),
            })
        };
        match node.kind {
            NodeKind::Connection => {
                let dbs = self.service.list_databases(conn_id()?).await?;
                Ok(dbs
                    .into_iter()
                    .map(|d| TreeNode {
                        connection_id: node.connection_id,
                        database: Some(d.name.clone()),
                        ..TreeNode::new(NodeKind::Database, d.name)
                    })
                    .collect())
            }
            NodeKind::Database => {
                let at = |kind| TreeNode {
                    connection_id: node.connection_id,
                    database: node.database.clone(),
                    ..TreeNode::new(
                        NodeKind::Folder(kind),
                        match kind {
                            FolderKind::Tables => "Tables",
                            FolderKind::Views => "Views",
                        },
                    )
                };
                Ok(vec![at(FolderKind::Tables), at(FolderKind::Views)])
            }
            NodeKind::Folder(kind) => {
                let id = conn_id()?;
                let db = node.database.as_deref().unwrap_or_default();
                // The flat tree has no schema level: one list_tables call
                // per schema, folded into schema.name rows.
                // ponytail: N+1 per schema — a wildcard LIST_TABLES query
                // collapses this if schema counts ever hurt.
                let schemas = self.service.list_schemas(&id, db).await?;
                let want = match kind {
                    FolderKind::Tables => TableKind::Base,
                    FolderKind::Views => TableKind::View,
                };
                let node_kind = match kind {
                    FolderKind::Tables => NodeKind::Table,
                    FolderKind::Views => NodeKind::View,
                };
                let mut out = Vec::new();
                for s in schemas {
                    for t in self.service.list_tables(&id, db, &s.name).await? {
                        if t.kind != want {
                            continue;
                        }
                        out.push(TreeNode {
                            connection_id: node.connection_id,
                            database: node.database.clone(),
                            schema: Some(t.schema.clone()),
                            table: Some(t.name.clone()),
                            ..TreeNode::new(node_kind, format!("{}.{}", t.schema, t.name))
                        });
                    }
                }
                Ok(out)
            }
            NodeKind::Table | NodeKind::View => {
                let desc = self
                    .service
                    .describe_table(
                        &conn_id()?,
                        node.database.as_deref().unwrap_or_default(),
                        node.schema.as_deref().unwrap_or_default(),
                        node.table.as_deref().unwrap_or_default(),
                    )
                    .await?;
                Ok(desc
                    .columns
                    .into_iter()
                    .map(|c| {
                        TreeNode::new(NodeKind::Column, format!("{}: {}", c.name, c.data_type))
                    })
                    .collect())
            }
            // Leaves never reach expand; unreachable via `expandable`.
            NodeKind::Column | NodeKind::Loading => Ok(vec![]),
        }
    }
}

#[cfg(test)]
mod tests {
    //! End-to-end smoke for the save flow: profile row + keyring item must
    //! persist together. Needs a session Secret Service, so it's ignored by
    //! default — run with:
    //!   DATARA_DATA_DIR=$(mktemp -d) cargo test -p datara -- --ignored

    use super::*;
    use crate::bridge::UiCtx;
    use parking_lot::Mutex;
    use secrecy::ExposeSecret;

    /// Void dispatch — the test asserts on storage, not the UI.
    fn test_ui(svc: &AppServices) -> UiHandle {
        UiHandle::detached(std::sync::Arc::new(UiCtx {
            tree: Mutex::new(crate::schema_tree::SchemaTree::default()),
            editor: Mutex::new(crate::editor_ui::EditorState::default()),
            backend: Arc::clone(&svc.backend),
            handle: svc.runtime.handle().clone(),
            query_limit: svc.config.query.default_limit,
        }))
    }

    #[test]
    fn preview_sql_quotes_and_substitutes_limit() {
        assert_eq!(
            preview_sql("dbo", "users", 500),
            "SELECT TOP 500 * FROM [dbo].[users]"
        );
        assert_eq!(
            preview_sql("a]b", "t]x", 1),
            "SELECT TOP 1 * FROM [a]]b].[t]]x]"
        );
    }

    #[test]
    fn query_error_text_formats_server_location() {
        let e = DomainError::Query {
            message: "Invalid object name 'foo'.".into(),
            error_number: Some(208),
            line: Some(3),
            column: None,
        };
        assert_eq!(
            query_error_text(&e),
            "Msg 208, Line 3: Invalid object name 'foo'."
        );

        let bare = DomainError::Query {
            message: "boom".into(),
            error_number: None,
            line: None,
            column: None,
        };
        assert_eq!(query_error_text(&bare), "Query failed: boom");

        let other = DomainError::Storage {
            message: "disk".into(),
        };
        assert_eq!(query_error_text(&other), "Storage error: disk");
    }

    #[test]
    #[ignore = "needs a live Secret Service; run manually"]
    fn smoke_save_persists_profile_and_secret() {
        let tmp = std::env::temp_dir().join(format!("datara-smoke-{}", std::process::id()));
        std::env::set_var("DATARA_DATA_DIR", &tmp);

        let svc = AppServices::init().expect("services init");
        let form = ConnForm {
            name: "smoke".into(),
            host: "127.0.0.1".into(),
            port: 11433,
            database: "master".into(),
            username: "sa".into(),
            password: "smoke-password".into(),
            encryption: "disabled".into(),
            trust_cert: true,
        };

        let (id, list) = svc.runtime.block_on(async {
            svc.backend.save_connection(form, test_ui(&svc)).await;
            let list = svc.backend.storage.connections().list().await.unwrap();
            let row = list.iter().find(|c| c.name == "smoke").unwrap().clone();
            let secret = svc
                .backend
                .secrets
                .load(&row.secret_reference)
                .await
                .unwrap();
            assert_eq!(secret.expose_secret(), "smoke-password");
            if std::env::var_os("DATARA_SMOKE_KEEP").is_none() {
                // Clean up: delete row + secret so real state stays untouched.
                svc.backend
                    .storage
                    .connections()
                    .delete(row.id)
                    .await
                    .unwrap();
                svc.backend
                    .secrets
                    .delete(&row.secret_reference)
                    .await
                    .unwrap();
            }
            (row.id, list.len())
        });

        assert!(id.0 > 0);
        assert_eq!(list, 1);
        if std::env::var_os("DATARA_SMOKE_KEEP").is_none() {
            std::fs::remove_dir_all(&tmp).ok();
        }
        println!("smoke data dir: {}", tmp.display());
    }

    /// Live end-to-end execute + cancel against the dev container
    /// (`datara-mssql` on 127.0.0.1:11433, sa / Datara!1234): seed a
    /// profile + secret, run SELECT (history row written), run a 30s
    /// WAITFOR, cancel it (abort + disconnect → no history row), then
    /// execute again on the fresh session.
    #[test]
    #[ignore = "needs datara-mssql on 127.0.0.1:11433 and a session Secret Service"]
    fn smoke_execute_then_cancel() {
        let tmp = std::env::temp_dir().join(format!("datara-exec-smoke-{}", std::process::id()));
        std::env::set_var("DATARA_DATA_DIR", &tmp);
        std::env::set_var("DATARA_CONFIG_DIR", tmp.join("cfg"));
        std::env::set_var("DATARA_STATE_DIR", tmp.join("state"));

        let svc = AppServices::init().expect("services init");
        svc.runtime.block_on(async {
            let backend = Arc::clone(&svc.backend);
            let repo = backend.storage.connections();
            let id = repo
                .insert(NewConnection {
                    name: "exec-smoke".into(),
                    host: "127.0.0.1".into(),
                    port: 11433,
                    database: Some("master".into()),
                    username: "sa".into(),
                    authentication: AuthenticationMode::SqlPassword,
                    encryption: EncryptionMode::Preferred,
                    trust_server_certificate: true,
                })
                .await
                .unwrap();
            let reference = repo.get(id).await.unwrap().secret_reference;
            backend
                .secrets
                .save(
                    &reference,
                    "exec-smoke",
                    &SecretString::from("Datara!1234".to_owned()),
                )
                .await
                .unwrap();

            let ui = test_ui(&svc);

            // Successful execute writes a history row (recorded inside the
            // task before the handle resolves).
            backend
                .execute_sql(
                    1,
                    id,
                    Some("master".into()),
                    "SELECT 1".into(),
                    10,
                    ui.clone(),
                )
                .await;
            let h = backend
                .storage
                .history()
                .search(Some(id), "SELECT", 10)
                .await
                .unwrap();
            assert_eq!(h.len(), 1);
            assert!(h[0].success);

            // Hanging query → marked running → cancel → history clean.
            let b2 = Arc::clone(&backend);
            let ui2 = ui.clone();
            let exec = tokio::spawn(async move {
                b2.execute_sql(2, id, None, "WAITFOR DELAY '00:00:30'".into(), 10, ui2)
                    .await;
            });
            for _ in 0..100 {
                if backend.any_running() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            assert!(backend.any_running(), "WAITFOR never registered");
            backend.cancel_query(2, ui.clone()).await;
            assert!(!backend.any_running());
            exec.await.unwrap();
            assert!(backend
                .storage
                .history()
                .search(Some(id), "WAITFOR", 10)
                .await
                .unwrap()
                .is_empty());

            // Post-cancel execute reconnects and works.
            backend
                .execute_sql(3, id, None, "SELECT 2".into(), 10, ui)
                .await;
            let h = backend
                .storage
                .history()
                .search(Some(id), "SELECT 2", 10)
                .await
                .unwrap();
            assert_eq!(h.len(), 1);

            backend.secrets.delete(&reference).await.unwrap();
            // The profile row stays: history FK-references it, and the
            // whole temp db is removed below anyway.
        });
        std::fs::remove_dir_all(&tmp).ok();
    }

    /// Exercises the real fetch chain end-to-end against a live server:
    /// connection → databases → Tables/Views folders → `schema.name` rows →
    /// columns. Seed `/tmp/datara-smoke-data/app.db` with a `smoke-mssql`
    /// profile (127.0.0.1:11433, sa) and its `mssql/<id>/password` secret.
    #[test]
    #[ignore = "needs a live MSSQL on 127.0.0.1:11433 and a seeded 'smoke-mssql' profile"]
    fn smoke_node_children_fetches_live_schema() {
        let data = std::env::temp_dir().join("datara-smoke-data");
        std::env::set_var("DATARA_DATA_DIR", &data);
        std::env::set_var(
            "DATARA_CONFIG_DIR",
            std::env::temp_dir().join("datara-smoke-config"),
        );
        std::env::set_var(
            "DATARA_STATE_DIR",
            std::env::temp_dir().join("datara-smoke-state"),
        );

        let svc = AppServices::init().expect("services init");
        svc.runtime.block_on(async {
            let conns = svc.backend.storage.connections().list().await.unwrap();
            let conn = conns
                .iter()
                .find(|c| c.name == "smoke-mssql")
                .expect("seed the smoke-mssql profile first")
                .clone();
            let mut node = TreeNode::new(NodeKind::Connection, conn.name.clone());
            node.connection_id = Some(conn.id);

            // Connection → databases.
            let dbs = svc.backend.node_children(&node).await.unwrap();
            assert!(dbs.iter().all(|d| d.kind == NodeKind::Database));
            let master = dbs
                .iter()
                .find(|d| d.label == "master")
                .expect("master database")
                .clone();

            // Database → Tables/Views folders.
            let folders = svc.backend.node_children(&master).await.unwrap();
            assert_eq!(
                folders.iter().map(|f| f.label.as_str()).collect::<Vec<_>>(),
                ["Tables", "Views"]
            );

            // Tables folder → schema.name rows across all schemas.
            let tables = svc.backend.node_children(&folders[0]).await.unwrap();
            assert!(tables.iter().all(|t| t.kind == NodeKind::Table));
            assert!(tables.iter().all(|t| t.label.contains('.')));
            // Views folder → only View-kind rows.
            let views = svc.backend.node_children(&folders[1]).await.unwrap();
            assert!(views.iter().all(|v| v.kind == NodeKind::View));

            // First table → leaf columns.
            if let Some(t) = tables.first() {
                let cols = svc.backend.node_children(t).await.unwrap();
                assert!(!cols.is_empty());
                assert!(cols
                    .iter()
                    .all(|c| c.kind == NodeKind::Column && !c.has_children));
            }
        });
    }
}
