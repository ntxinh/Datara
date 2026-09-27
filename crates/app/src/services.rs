//! Process-wide services: Tokio runtime, storage, secret store, and the
//! shared [`DatabaseService`]. Created once on startup, then shared with the
//! Slint UI as `Arc<AppServices>`; backend work is spawned onto `runtime`
//! from UI callbacks (see `ui.rs`), results return via `UiHandle`.

use std::sync::Arc;

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
        let backend = Arc::new(Backend {
            storage,
            secrets,
            service,
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

    /// `toggle-node` on an unexpanded row: fetch its children by kind and
    /// dispatch them back for `SchemaTree::replace_children`.
    pub async fn expand_node(&self, node: TreeNode, ui: UiHandle) {
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
    use secrecy::ExposeSecret;

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
            svc.backend.save_connection(form, UiHandle::default()).await;
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
