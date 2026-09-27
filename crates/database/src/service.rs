//! [`DatabaseService`]: the shared database access layer used by the UI
//! bridge and the MCP server. It owns a session pool keyed by
//! [`ConnectionId`], resolves passwords from the secret store via each
//! profile's `secret_reference`, and records query history on every
//! `execute` completion.
//!
//! The service is generic over the driver because `datara-driver-mssql`
//! already depends on this crate for the traits — a concrete `MssqlDriver`
//! field would make Cargo's dependency graph a cycle. `D::default()` in
//! [`DatabaseService::new`] is zero-cost (`MssqlDriver` is a unit struct).

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use datara_domain::{
    ConnectionId, ConnectionProfile, Credentials, DatabaseInfo, QueryHistoryEntry, QueryResult,
    Result, SchemaInfo, TableDescription, TableInfo,
};
use datara_storage::Storage;
use secrecy::SecretString;
use tokio::sync::Mutex;

use crate::{async_trait, DatabaseDriver, DatabaseSession};

/// Handle to a query running on a spawned task. Await it for the result;
/// call [`tokio::task::JoinHandle::abort`] to cancel — aborting drops the
/// in-flight query future and its borrow of the client's TDS stream, so the
/// connection becomes unusable (the session pool should be reset on cancel).
/// A cancelled query records no history entry.
pub type QueryHandle = tokio::task::JoinHandle<Result<QueryResult>>;

/// Source for the secret referenced by a connection profile. Production uses
/// [`datara_secrets::SecretStore`]; tests substitute a stub.
#[async_trait]
pub trait SecretSource: Send + Sync {
    /// Load the secret stored for `reference`.
    async fn load(&self, reference: &datara_domain::SecretReference) -> Result<SecretString>;
}

#[async_trait]
impl SecretSource for datara_secrets::SecretStore {
    async fn load(&self, reference: &datara_domain::SecretReference) -> Result<SecretString> {
        // Inherent method wins over this trait — qualify to be explicit.
        datara_secrets::SecretStore::load(self, reference).await
    }
}

/// Shared database access: lazy session pool, credential resolution, and
/// query-history recording. Cheap to share — clonable `Arc`s inside.
pub struct DatabaseService<D: DatabaseDriver> {
    storage: Arc<Storage>,
    secrets: Arc<dyn SecretSource>,
    driver: D,
    sessions: Mutex<HashMap<ConnectionId, Arc<dyn DatabaseSession>>>,
}

impl<D: DatabaseDriver + Default> DatabaseService<D> {
    /// Create the service. `secrets` is the secret-source used to resolve
    /// each profile's `secret_reference` at connect time.
    pub fn new<S: SecretSource + 'static>(storage: Arc<Storage>, secrets: Arc<S>) -> Self {
        Self {
            storage,
            secrets,
            driver: D::default(),
            sessions: Mutex::new(HashMap::new()),
        }
    }
}

impl<D: DatabaseDriver> DatabaseService<D> {
    /// Connect with `creds`, run no query, and drop the session — nothing is
    /// cached. Used by the connection dialog's Test button.
    pub async fn test_connection(
        &self,
        profile: &ConnectionProfile,
        creds: &Credentials,
    ) -> Result<()> {
        self.driver.connect(profile, creds).await.map(|_| ())
    }

    /// Get the cached session for `id`, or connect using the stored profile
    /// and its referenced secret.
    ///
    /// The map lock is held only for lookup/insert; the connect runs unlocked.
    /// ponytail: two concurrent connects for the same id race and the last
    /// insert wins — benign; the loser is dropped.
    pub async fn session(&self, id: ConnectionId) -> Result<Arc<dyn DatabaseSession>> {
        if let Some(session) = self.sessions.lock().await.get(&id).cloned() {
            return Ok(session);
        }
        let profile = self.storage.connections().get(id).await?;
        let password = self.secrets.load(&profile.secret_reference).await?;
        let creds = Credentials {
            username: profile.username.clone(),
            password,
        };
        let session: Arc<dyn DatabaseSession> =
            Arc::from(self.driver.connect(&profile, &creds).await?);
        let mut sessions = self.sessions.lock().await;
        Ok(sessions.entry(id).or_insert(session).clone())
    }

    /// Drop the cached session for `id`, if any. The next `session(id)`
    /// reconnects.
    pub async fn disconnect(&self, id: ConnectionId) {
        self.sessions.lock().await.remove(&id);
    }

    /// Databases visible to the session for `id`.
    pub async fn list_databases(&self, id: ConnectionId) -> Result<Vec<DatabaseInfo>> {
        self.session(id).await?.list_databases().await
    }

    /// Schemas inside `database`.
    pub async fn list_schemas(&self, id: &ConnectionId, database: &str) -> Result<Vec<SchemaInfo>> {
        self.session(*id).await?.list_schemas(database).await
    }

    /// Tables/views inside `database`.`schema`.
    pub async fn list_tables(
        &self,
        id: &ConnectionId,
        database: &str,
        schema: &str,
    ) -> Result<Vec<TableInfo>> {
        self.session(*id).await?.list_tables(database, schema).await
    }

    /// Describe `database`.`schema`.`table`.
    pub async fn describe_table(
        &self,
        id: &ConnectionId,
        database: &str,
        schema: &str,
        table: &str,
    ) -> Result<TableDescription> {
        self.session(*id)
            .await?
            .describe_table(database, schema, table)
            .await
    }

    /// Run `query` on a spawned task; history is recorded inside that task on
    /// completion (duration, row count, success, error message).
    ///
    /// `database` falls back to the profile's configured database, then to
    /// `master`. Aborting the returned handle drops the in-flight query —
    /// see [`QueryHandle`].
    pub async fn execute(
        &self,
        id: ConnectionId,
        database: Option<String>,
        query: String,
        max_rows: usize,
    ) -> Result<QueryHandle> {
        let session = self.session(id).await?;
        let database = match database {
            Some(db) => db,
            None => self
                .storage
                .connections()
                .get(id)
                .await?
                .database
                .unwrap_or_else(|| "master".to_owned()),
        };
        let storage = Arc::clone(&self.storage);
        Ok(tokio::spawn(async move {
            let started_at = jiff::Timestamp::now();
            let started = Instant::now();
            let result = session.execute(&database, &query, max_rows).await;
            let (row_count, error_message) = match &result {
                // row_count: rows returned for selects, rows affected otherwise.
                Ok(r) => (r.rows_affected.unwrap_or(r.rows.len() as u64), None),
                Err(e) => (0, Some(e.to_string())),
            };
            let entry = QueryHistoryEntry {
                id: 0,
                connection_id: id,
                database: Some(database),
                query,
                started_at,
                duration_ms: started.elapsed().as_millis() as u64,
                row_count,
                success: result.is_ok(),
                error_message,
            };
            if let Err(e) = storage.history().record(&entry).await {
                tracing::warn!("failed to record query history: {e}");
            }
            result
        }))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use datara_domain::{AuthenticationMode, EncryptionMode};
    use datara_domain::{DomainError, SecretReference};
    use datara_storage::NewConnection;

    use super::*;

    struct StubSecrets;

    #[async_trait]
    impl SecretSource for StubSecrets {
        async fn load(&self, _reference: &SecretReference) -> Result<SecretString> {
            Ok(SecretString::from("hunter2"))
        }
    }

    /// Session that returns a fixed query result (or a fixed error when
    /// `fail`; a never-completing future when `hang`). Metadata methods
    /// return empty vecs — unused by these tests.
    struct StubSession {
        fail: bool,
        hang: bool,
    }

    fn one_result() -> QueryResult {
        QueryResult {
            columns: vec![datara_domain::QueryColumn {
                name: "one".into(),
                data_type: "int".into(),
            }],
            rows: vec![datara_domain::QueryRow {
                cells: vec![datara_domain::Value::Int(1)],
            }],
            rows_affected: None,
            truncated: false,
        }
    }

    #[async_trait]
    impl DatabaseSession for StubSession {
        async fn list_databases(&self) -> Result<Vec<DatabaseInfo>> {
            Ok(vec![DatabaseInfo {
                name: "master".into(),
            }])
        }
        async fn list_schemas(&self, _d: &str) -> Result<Vec<SchemaInfo>> {
            Ok(vec![])
        }
        async fn list_tables(&self, _d: &str, _s: &str) -> Result<Vec<TableInfo>> {
            Ok(vec![])
        }
        async fn describe_table(&self, d: &str, s: &str, t: &str) -> Result<TableDescription> {
            Ok(TableDescription {
                schema: s.into(),
                name: format!("{d}.{t}"),
                columns: vec![],
                indexes: vec![],
            })
        }
        async fn execute(&self, _d: &str, _q: &str, _m: usize) -> Result<QueryResult> {
            if self.hang {
                std::future::pending::<()>().await;
            }
            if self.fail {
                Err(DomainError::Query {
                    message: "boom".into(),
                    error_number: None,
                    line: None,
                    column: None,
                })
            } else {
                Ok(one_result())
            }
        }
        async fn cancel(&self) -> Result<()> {
            Ok(())
        }
        fn quote_ident(&self, ident: &str) -> String {
            format!("[{ident}]")
        }
    }

    /// Driver returning `StubSession`s; counts connects so tests can assert
    /// session caching.
    struct StubDriver {
        connects: Arc<AtomicUsize>,
        fail: bool,
        hang: bool,
        /// When set, `connect` parks on this notify — lets a test pause a
        /// connect mid-flight (cancel-during-connect races).
        gate: Option<Arc<tokio::sync::Notify>>,
    }

    #[async_trait]
    impl DatabaseDriver for StubDriver {
        async fn connect(
            &self,
            _p: &ConnectionProfile,
            _c: &Credentials,
        ) -> Result<Box<dyn DatabaseSession>> {
            self.connects.fetch_add(1, Ordering::SeqCst);
            if let Some(gate) = &self.gate {
                gate.notified().await;
            }
            Ok(Box::new(StubSession {
                fail: self.fail,
                hang: self.hang,
            }))
        }
    }

    async fn service(
        fail: bool,
        hang: bool,
    ) -> (
        DatabaseService<StubDriver>,
        Arc<AtomicUsize>,
        ConnectionId,
        tempfile::TempDir,
    ) {
        service_opts(fail, hang, None).await
    }

    async fn service_opts(
        fail: bool,
        hang: bool,
        gate: Option<Arc<tokio::sync::Notify>>,
    ) -> (
        DatabaseService<StubDriver>,
        Arc<AtomicUsize>,
        ConnectionId,
        tempfile::TempDir,
    ) {
        let dir = tempfile::tempdir().unwrap();
        let storage = Arc::new(Storage::open(&dir.path().join("test.db")).await.unwrap());
        let id = storage
            .connections()
            .insert(NewConnection {
                name: "it".into(),
                host: "127.0.0.1".into(),
                port: 1433,
                database: None,
                username: "sa".into(),
                authentication: AuthenticationMode::SqlPassword,
                encryption: EncryptionMode::Preferred,
                trust_server_certificate: true,
            })
            .await
            .unwrap();
        let connects = Arc::new(AtomicUsize::new(0));
        let svc = DatabaseService {
            storage,
            secrets: Arc::new(StubSecrets),
            driver: StubDriver {
                connects: Arc::clone(&connects),
                fail,
                hang,
                gate,
            },
            sessions: Mutex::new(HashMap::new()),
        };
        (svc, connects, id, dir)
    }

    #[tokio::test]
    async fn session_connects_once_and_caches() {
        let (svc, connects, id, _dir) = service(false, false).await;
        let a = svc.session(id).await.unwrap();
        let b = svc.session(id).await.unwrap();
        assert!(Arc::ptr_eq(&a, &b));
        assert_eq!(connects.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn disconnect_reconnects() {
        let (svc, connects, id, _dir) = service(false, false).await;
        svc.session(id).await.unwrap();
        svc.disconnect(id).await;
        svc.session(id).await.unwrap();
        assert_eq!(connects.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn session_unknown_id_is_storage_error() {
        let (svc, _c, _id, _dir) = service(false, false).await;
        let err = svc.session(ConnectionId(999)).await.err().unwrap();
        assert!(matches!(err, DomainError::Storage { .. }));
    }

    #[tokio::test]
    async fn test_connection_does_not_cache() {
        let (svc, connects, id, _dir) = service(false, false).await;
        let profile = svc.storage.connections().get(id).await.unwrap();
        let creds = Credentials {
            username: "sa".into(),
            password: SecretString::from("x"),
        };
        svc.test_connection(&profile, &creds).await.unwrap();
        assert!(svc.sessions.lock().await.is_empty());
        assert_eq!(connects.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn execute_records_success_history() {
        let (svc, _c, id, _dir) = service(false, false).await;
        let result = svc
            .execute(id, None, "SELECT 1".into(), 10)
            .await
            .unwrap()
            .await
            .unwrap()
            .unwrap();
        assert_eq!(result.columns[0].name, "one");

        let history = svc
            .storage
            .history()
            .search(Some(id), "SELECT", 10)
            .await
            .unwrap();
        assert_eq!(history.len(), 1);
        let entry = &history[0];
        assert!(entry.success);
        assert_eq!(entry.row_count, 1);
        assert_eq!(entry.database.as_deref(), Some("master"));
        assert!(entry.error_message.is_none());
    }

    #[tokio::test]
    async fn execute_records_failure_history() {
        let (svc, _c, id, _dir) = service(true, false).await;
        let err = svc
            .execute(id, Some("db".into()), "SELECT bad".into(), 10)
            .await
            .unwrap()
            .await
            .unwrap()
            .unwrap_err();
        assert!(matches!(err, DomainError::Query { .. }));

        let history = svc
            .storage
            .history()
            .search(Some(id), "", 10)
            .await
            .unwrap();
        assert_eq!(history.len(), 1);
        assert!(!history[0].success);
        assert_eq!(history[0].database.as_deref(), Some("db"));
        assert!(history[0].error_message.is_some());
    }

    /// Cancel path used by the UI: abort the in-flight handle, then
    /// `disconnect` — the aborted query left the TDS stream mid-response,
    /// so the pooled session is dropped and the next call reconnects.
    /// A cancelled query records no history entry.
    #[tokio::test]
    async fn abort_then_disconnect_resets_session() {
        let (svc, connects, id, _dir) = service(false, true).await;
        let handle = svc
            .execute(id, None, "WAITFOR DELAY".into(), 10)
            .await
            .unwrap();
        assert_eq!(connects.load(Ordering::SeqCst), 1);

        handle.abort();
        assert!(handle.await.unwrap_err().is_cancelled());
        svc.disconnect(id).await;

        // Fresh session on next use — the desynced one is gone.
        svc.session(id).await.unwrap();
        assert_eq!(connects.load(Ordering::SeqCst), 2);

        // Cancelled queries record nothing.
        assert!(svc
            .storage
            .history()
            .search(Some(id), "", 10)
            .await
            .unwrap()
            .is_empty());
    }

    /// Cancel-during-connect ordering: `execute` awaits `session()` (which
    /// inserts into the pool) *before* returning its handle, so a caller
    /// that aborts the fresh handle and disconnects again always evicts the
    /// late-inserted session. Simulates `Backend::execute_sql`'s
    /// slot-vanished branch.
    #[tokio::test]
    async fn late_inserted_session_is_evicted() {
        let gate = Arc::new(tokio::sync::Notify::new());
        let (svc, connects, id, _dir) = service_opts(false, true, Some(gate.clone())).await;
        let svc = Arc::new(svc);

        // Spawn execute; it parks inside connect on the gate.
        let svc2 = Arc::clone(&svc);
        let exec = tokio::spawn(async move { svc2.execute(id, None, "WAITFOR".into(), 10).await });
        for _ in 0..100 {
            if connects.load(Ordering::SeqCst) == 1 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }

        // "Cancel" while the connect is still in flight.
        svc.disconnect(id).await;
        gate.notify_one();
        let handle = exec.await.unwrap().unwrap();

        // What execute_sql does when its slot vanished: abort, then
        // disconnect — the session inserted during the await is evicted.
        handle.abort();
        let _ = handle.await;
        svc.disconnect(id).await;
        assert!(svc.sessions.lock().await.is_empty());

        // Next use reconnects instead of reusing the abandoned session.
        gate.notify_one();
        svc.session(id).await.unwrap();
        assert_eq!(connects.load(Ordering::SeqCst), 2);
    }
}
