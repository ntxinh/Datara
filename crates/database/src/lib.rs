//! Database driver abstraction traits and the shared [`DatabaseService`].
//!
//! This crate is the seam between the domain layer and concrete drivers
//! (e.g. the MSSQL driver). The `list_*` methods take a `database` argument
//! because MSSQL requires a database context for metadata queries; `execute`
//! takes `max_rows` so that drivers must cap row materialization (spec §11).

mod service;

use async_trait::async_trait;
use datara_domain::{
    ConnectionProfile, Credentials, DatabaseInfo, QueryResult, Result, SchemaInfo,
    TableDescription, TableInfo,
};

/// A database driver capable of opening sessions against a connection profile.
#[async_trait]
pub trait DatabaseDriver: Send + Sync {
    /// Open a new session for `profile` authenticated with `credentials`.
    async fn connect(
        &self,
        profile: &ConnectionProfile,
        credentials: &Credentials,
    ) -> Result<Box<dyn DatabaseSession>>;
}

/// An open database session. Sessions are `Send + Sync` so a single session
/// can be shared across tasks via `Arc`; each driver serializes access
/// internally.
#[async_trait]
pub trait DatabaseSession: Send + Sync {
    /// List databases visible to this session.
    async fn list_databases(&self) -> Result<Vec<DatabaseInfo>>;

    /// List schemas inside `database` (MSSQL requires the database context).
    async fn list_schemas(&self, database: &str) -> Result<Vec<SchemaInfo>>;

    /// List tables inside `database`.`schema`.
    async fn list_tables(&self, database: &str, schema: &str) -> Result<Vec<TableInfo>>;

    /// Describe `database`.`schema`.`table` (columns, indexes, etc.).
    async fn describe_table(
        &self,
        database: &str,
        schema: &str,
        table: &str,
    ) -> Result<TableDescription>;

    /// Execute `query` against `database`, materializing at most `max_rows`.
    /// Drivers must enforce the cap, not the caller.
    async fn execute(&self, database: &str, query: &str, max_rows: usize) -> Result<QueryResult>;

    /// Cancel the currently running statement, if any.
    async fn cancel(&self) -> Result<()>;

    /// Quote `ident` as a delimited identifier using this driver's dialect.
    /// There is intentionally no default impl: quoting is per-driver.
    /// MSSQL convention: wrap in `[` … `]` and escape `]` as `]]`, so
    /// `a]b` becomes `[a]]b]`.
    fn quote_ident(&self, ident: &str) -> String;
}

pub use service::{DatabaseService, QueryHandle, SecretSource};

#[cfg(test)]
mod tests {
    //! Demonstrates the `quote_ident` contract shape and trait object safety.
    //! The real MSSQL implementation lands in the driver crate (Task 2.2).

    use super::*;
    use datara_domain::{ColumnInfo, IndexInfo};

    struct MssqlShape;

    #[async_trait]
    impl DatabaseSession for MssqlShape {
        async fn list_databases(&self) -> Result<Vec<DatabaseInfo>> {
            Ok(vec![])
        }

        async fn list_schemas(&self, _database: &str) -> Result<Vec<SchemaInfo>> {
            Ok(vec![])
        }

        async fn list_tables(&self, _database: &str, _schema: &str) -> Result<Vec<TableInfo>> {
            Ok(vec![])
        }

        async fn describe_table(
            &self,
            _database: &str,
            schema: &str,
            table: &str,
        ) -> Result<TableDescription> {
            Ok(TableDescription {
                schema: schema.to_string(),
                name: table.to_string(),
                columns: Vec::<ColumnInfo>::new(),
                indexes: Vec::<IndexInfo>::new(),
            })
        }

        async fn execute(
            &self,
            _database: &str,
            _query: &str,
            _max_rows: usize,
        ) -> Result<QueryResult> {
            Ok(QueryResult {
                columns: vec![],
                rows: vec![],
                rows_affected: None,
                truncated: false,
            })
        }

        async fn cancel(&self) -> Result<()> {
            Ok(())
        }

        fn quote_ident(&self, ident: &str) -> String {
            format!("[{}]", ident.replace(']', "]]"))
        }
    }

    #[test]
    fn quote_ident_wraps_and_escapes() {
        let s = MssqlShape;
        assert_eq!(s.quote_ident("users"), "[users]");
        assert_eq!(s.quote_ident("a]b"), "[a]]b]");
    }

    #[test]
    fn session_is_object_safe() {
        fn assert_obj(_: &dyn DatabaseSession) {}
        let session: Box<dyn DatabaseSession> = Box::new(MssqlShape);
        assert_obj(session.as_ref());
        assert_eq!(session.quote_ident("t"), "[t]");
    }
}
