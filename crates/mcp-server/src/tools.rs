//! Tool implementations. Each `#[tool]` method maps one MCP tool onto a
//! [`DatabaseService`] call and returns the domain type serialized as a
//! JSON text block; errors surface as tool-level error content so the
//! model sees the message rather than a transport failure.

use datara_database::DatabaseDriver;
use datara_domain::{ConnectionId, ConnectionProfile, TableInfo, TableKind, Value};
use rmcp::{
    handler::server::wrapper::Parameters,
    model::{CallToolResult, ContentBlock},
    tool, tool_router, ErrorData as McpError,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sqlparser::ast::{Query, SetExpr, Statement, TableFactor};
use sqlparser::dialect::GenericDialect;
use sqlparser::parser::Parser;

use crate::server::{cap_rows, DataraMcp, DEFAULT_SCHEMA};

/// Serialize `value` as a JSON text block.
fn json_result(value: impl Serialize) -> Result<CallToolResult, McpError> {
    match serde_json::to_string(&value) {
        Ok(json) => Ok(CallToolResult::success(vec![ContentBlock::text(json)])),
        Err(e) => Ok(CallToolResult::error(vec![ContentBlock::text(format!(
            "serialization failed: {e}"
        ))])),
    }
}

/// Domain errors become tool-error content, not JSON-RPC protocol errors —
/// the model reads the message and can react.
fn tool_err(e: impl std::fmt::Display) -> CallToolResult {
    CallToolResult::error(vec![ContentBlock::text(e.to_string())])
}
/// Wrap a row-producing result as `{ <name>: [...], truncated: bool }`,
/// truncating to `max` first. Same convention `execute_query` uses via
/// `QueryResult.truncated` — a capped list must not look complete.
fn capped_result(
    name: &'static str,
    mut items: Vec<impl Serialize>,
    max: usize,
) -> Result<CallToolResult, McpError> {
    let truncated = items.len() > max;
    items.truncate(max);
    json_result(serde_json::json!({ name: items, "truncated": truncated }))
}

/// `list_connections` output: the profile minus `secret_reference` and
/// auth/encryption internals — nothing here can carry a password.
#[derive(Debug, Serialize)]
struct ConnectionSummary {
    id: i64,
    name: String,
    host: String,
    port: u16,
    database: Option<String>,
}

impl From<ConnectionProfile> for ConnectionSummary {
    fn from(p: ConnectionProfile) -> Self {
        Self {
            id: p.id.0,
            name: p.name,
            host: p.host,
            port: p.port,
            database: p.database,
        }
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ConnectionArgs {
    /// Connection id from `list_connections`.
    pub conn_id: i64,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct TablesArgs {
    /// Connection id from `list_connections`.
    pub conn_id: i64,
    /// Database name; defaults to the connection's configured database.
    pub database: Option<String>,
    /// Schema name; defaults to "dbo".
    pub schema: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct DescribeArgs {
    /// Connection id from `list_connections`.
    pub conn_id: i64,
    /// Database name; defaults to the connection's configured database.
    pub database: Option<String>,
    /// Schema name; defaults to "dbo".
    pub schema: Option<String>,
    /// Table or view name.
    pub table: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SearchArgs {
    /// Connection id from `list_connections`.
    pub conn_id: i64,
    /// Table/view name fragment; SQL LIKE wildcards % and _ work.
    pub query: String,
    /// Database name; defaults to the connection's configured database.
    pub database: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct QueryArgs {
    /// Connection id from `list_connections`.
    pub conn_id: i64,
    /// SQL text (T-SQL).
    pub query: String,
    /// Database name; defaults to the connection's configured database.
    pub database: Option<String>,
}

/// Whether the SQL batch is provably read-only: every statement parses to a
/// `SELECT` query AND `query_is_read` verifies no nested write (`SELECT
/// INTO`, `VALUES`-fed `INSERT`, CTE bodies, derived tables). Unparseable
/// SQL is NOT read-only — the parser never vouches for what it couldn't read
/// (mirrors `is_row_returning` in the MSSQL driver, fail-safe flipped).
fn is_read_only(query: &str) -> bool {
    match Parser::parse_sql(&GenericDialect {}, query) {
        Ok(stmts) => {
            !stmts.is_empty()
                && stmts
                    .iter()
                    .all(|s| matches!(s, Statement::Query(q) if query_is_read(q)))
        }
        Err(_) => false,
    }
}

/// Whether `q` is a pure read: every `Select` has no `INTO` target and every
/// nested `SetExpr`/CTE/derived table is read-only. `SetExpr` can carry
/// `Insert`/`Update`/`Delete`/`Merge`, so a shallow `is_select` is not proof.
fn query_is_read(q: &Query) -> bool {
    let ctes_read = q
        .with
        .as_ref()
        .is_none_or(|w| w.cte_tables.iter().all(|c| query_is_read(&c.query)));
    ctes_read && set_expr_is_read(&q.body)
}

fn set_expr_is_read(e: &SetExpr) -> bool {
    match e {
        // `SELECT ... INTO` writes; `select.into` must be absent.
        SetExpr::Select(s) => {
            s.into.is_none()
                && s.from.iter().all(|t| {
                    table_factor_is_read(&t.relation)
                        && t.joins.iter().all(|j| table_factor_is_read(&j.relation))
                })
        }
        SetExpr::Query(q) => query_is_read(q),
        SetExpr::SetOperation { left, right, .. } => {
            set_expr_is_read(left) && set_expr_is_read(right)
        }
        // Values/Table are read-only; Insert/Update/Delete/Merge are not.
        SetExpr::Values(_) | SetExpr::Table(_) => true,
        _ => false,
    }
}

fn table_factor_is_read(t: &TableFactor) -> bool {
    match t {
        // Bare table name (args: None). `args: Some` means a table-valued
        // function call — refused below with the other function factors.
        TableFactor::Table { args: None, .. } => true,
        TableFactor::Derived { subquery, .. } => query_is_read(subquery),
        TableFactor::NestedJoin {
            table_with_joins, ..
        } => {
            table_factor_is_read(&table_with_joins.relation)
                && table_with_joins
                    .joins
                    .iter()
                    .all(|j| table_factor_is_read(&j.relation))
        }
        TableFactor::Pivot { table, .. } | TableFactor::Unpivot { table, .. } => {
            table_factor_is_read(table)
        }
        // Anything else — TVFs (`Table{args:Some}`/`Function`/`TableFunction`),
        // `OPENQUERY`/`OPENROWSET`/`OPENXML`, `UNNEST`, `JSON_TABLE`, and every
        // factor not itemized — is refused. TVFs can carry remote writes
        // (OPENQUERY) or connection strings (OPENROWSET); the gate errs closed.
        _ => false,
    }
}

/// The `search_schema` metadata query: tables/views whose name contains
/// `query`. `'` is doubled for the SQL literal and `[` is escaped for LIKE
/// (it would otherwise open a character class); `%`/`_` stay live as
/// wildcards — a documented feature.
pub(crate) fn search_sql(query: &str) -> String {
    let needle = query.replace('\'', "''").replace('[', "[[]");
    format!(
        "SELECT s.name AS schema_name, o.name, CASE o.type WHEN 'V' THEN 'view' \
         ELSE 'table' END AS kind FROM sys.objects o JOIN sys.schemas s \
         ON o.schema_id = s.schema_id WHERE o.type IN ('U','V') \
         AND o.name LIKE '%{needle}%' ORDER BY s.name, o.name"
    )
}

#[tool_router(vis = "pub(crate)")]
impl<D: DatabaseDriver + 'static> DataraMcp<D> {
    /// Saved connection profiles — ids for every other tool's `conn_id`.
    /// No secrets are included.
    #[tool(
        description = "List saved Datara connection profiles (id, name, host, port, database). Use these ids as conn_id in other tools."
    )]
    async fn list_connections(&self) -> Result<CallToolResult, McpError> {
        match self.storage.connections().list().await {
            Ok(profiles) => capped_result(
                "connections",
                profiles
                    .into_iter()
                    .map(ConnectionSummary::from)
                    .collect::<Vec<_>>(),
                self.max_rows,
            ),
            Err(e) => Ok(tool_err(e)),
        }
    }

    #[tool(description = "List databases visible to a connection.")]
    async fn list_databases(
        &self,
        Parameters(args): Parameters<ConnectionArgs>,
    ) -> Result<CallToolResult, McpError> {
        match self
            .service
            .list_databases(ConnectionId(args.conn_id))
            .await
        {
            Ok(dbs) => capped_result("databases", dbs, self.max_rows),
            Err(e) => Ok(tool_err(e)),
        }
    }

    #[tool(description = "List tables and views in a schema of a database.")]
    async fn list_tables(
        &self,
        Parameters(args): Parameters<TablesArgs>,
    ) -> Result<CallToolResult, McpError> {
        let id = ConnectionId(args.conn_id);
        let schema = args.schema.unwrap_or_else(|| DEFAULT_SCHEMA.to_owned());
        match self.resolve_database(id, args.database.as_deref()).await {
            Err(e) => Ok(tool_err(e)),
            Ok(database) => match self.service.list_tables(&id, &database, &schema).await {
                Ok(tables) => capped_result("tables", tables, self.max_rows),
                Err(e) => Ok(tool_err(e)),
            },
        }
    }

    #[tool(
        description = "Describe a table or view: columns (name, type, nullable, pk) and indexes."
    )]
    async fn describe_table(
        &self,
        Parameters(args): Parameters<DescribeArgs>,
    ) -> Result<CallToolResult, McpError> {
        let id = ConnectionId(args.conn_id);
        let schema = args.schema.unwrap_or_else(|| DEFAULT_SCHEMA.to_owned());
        match self.resolve_database(id, args.database.as_deref()).await {
            Err(e) => Ok(tool_err(e)),
            Ok(database) => match self
                .service
                .describe_table(&id, &database, &schema, &args.table)
                .await
            {
                Ok(mut desc) => {
                    let truncated =
                        desc.columns.len() > self.max_rows || desc.indexes.len() > self.max_rows;
                    desc.columns.truncate(self.max_rows);
                    desc.indexes.truncate(self.max_rows);
                    json_result(serde_json::json!({
                        "table": desc,
                        "truncated": truncated,
                    }))
                }
                Err(e) => Ok(tool_err(e)),
            },
        }
    }

    /// Searches `sys.objects` by name — one query, no per-schema fan-out.
    /// Runs through `session.execute` directly so the metadata lookup is not
    /// recorded in the user's query history.
    #[tool(
        description = "Search table and view names containing `query` in a database. Returns {tables: [{schema, name, kind}], truncated: bool}. SQL LIKE wildcards % and _ work in query; [ is escaped."
    )]
    async fn search_schema(
        &self,
        Parameters(args): Parameters<SearchArgs>,
    ) -> Result<CallToolResult, McpError> {
        let id = ConnectionId(args.conn_id);
        let database = match self.resolve_database(id, args.database.as_deref()).await {
            Err(e) => return Ok(tool_err(e)),
            Ok(db) => db,
        };
        let session = match self.service.session(id).await {
            Ok(s) => s,
            Err(e) => return Ok(tool_err(e)),
        };
        let sql = search_sql(&args.query);
        match session.execute(&database, &sql, self.max_rows).await {
            Err(e) => Ok(tool_err(e)),
            Ok(mut result) => {
                cap_rows(&mut result, self.max_rows);
                let tables: Vec<TableInfo> = result
                    .rows
                    .iter()
                    .filter_map(|r| match r.cells.as_slice() {
                        [Value::Text(schema), Value::Text(name), Value::Text(kind)] => {
                            Some(TableInfo {
                                schema: schema.clone(),
                                name: name.clone(),
                                kind: if kind == "view" {
                                    TableKind::View
                                } else {
                                    TableKind::Base
                                },
                            })
                        }
                        _ => None,
                    })
                    .collect();
                json_result(serde_json::json!({
                    "tables": tables,
                    "truncated": result.truncated,
                }))
            }
        }
    }

    #[tool(
        description = "Execute a SQL query on a connection. Returns statement_type (read|write), columns, rows, rows_affected, truncated. Rows are capped by server config (mcp.max_result_rows); non-SELECT statements are refused unless mcp.allow_writes is set."
    )]
    async fn execute_query(
        &self,
        Parameters(args): Parameters<QueryArgs>,
    ) -> Result<CallToolResult, McpError> {
        let read_only = is_read_only(&args.query);
        if !read_only && !self.allow_writes {
            return Ok(tool_err(
                "write queries disabled — set mcp.allow_writes = true in config to allow",
            ));
        }
        match self
            .service
            .execute(
                ConnectionId(args.conn_id),
                args.database,
                args.query,
                self.max_rows,
            )
            .await
        {
            Err(e) => Ok(tool_err(e)),
            Ok(handle) => match handle.await {
                Err(join_err) => Ok(tool_err(join_err)),
                Ok(Err(e)) => Ok(tool_err(e)),
                Ok(Ok(mut result)) => {
                    cap_rows(&mut result, self.max_rows);
                    let row_count = result.rows.len();
                    json_result(serde_json::json!({
                        "statement_type": if read_only { "read" } else { "write" },
                        "columns": result.columns,
                        "rows": result.rows,
                        "row_count": row_count,
                        "rows_affected": result.rows_affected,
                        "truncated": result.truncated,
                    }))
                }
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use async_trait::async_trait;
    use datara_database::{DatabaseDriver, DatabaseService, DatabaseSession, SecretSource};
    use datara_domain::{
        AuthenticationMode, ColumnInfo, Credentials, DatabaseInfo, EncryptionMode, IndexInfo,
        QueryColumn, QueryResult, Result as DomainResult, SchemaInfo, SecretReference,
        TableDescription,
    };
    use datara_storage::{NewConnection, Storage};
    use secrecy::SecretString;

    use super::*;
    use crate::server::DEFAULT_SCHEMA;

    struct StubSecrets;

    #[async_trait]
    impl SecretSource for StubSecrets {
        async fn load(&self, _reference: &SecretReference) -> DomainResult<SecretString> {
            Ok(SecretString::from("hunter2"))
        }
    }

    /// Session returning fixed metadata and one [schema, name, kind] rowset
    /// for `execute`, matching `search_schema`'s SELECT shape.
    struct StubSession;

    #[async_trait]
    impl DatabaseSession for StubSession {
        async fn list_databases(&self) -> DomainResult<Vec<DatabaseInfo>> {
            Ok(vec![
                DatabaseInfo {
                    name: "master".into(),
                },
                DatabaseInfo {
                    name: "appdb".into(),
                },
            ])
        }
        async fn list_schemas(&self, _d: &str) -> DomainResult<Vec<SchemaInfo>> {
            Ok(vec![])
        }
        async fn list_tables(&self, _d: &str, s: &str) -> DomainResult<Vec<TableInfo>> {
            Ok(vec![TableInfo {
                schema: s.into(),
                name: "users".into(),
                kind: TableKind::Base,
            }])
        }
        async fn describe_table(
            &self,
            _d: &str,
            s: &str,
            t: &str,
        ) -> DomainResult<TableDescription> {
            Ok(TableDescription {
                schema: s.into(),
                name: t.into(),
                columns: vec![ColumnInfo {
                    name: "id".into(),
                    data_type: "int".into(),
                    nullable: false,
                    is_primary_key: true,
                    ordinal: 1,
                }],
                indexes: Vec::<IndexInfo>::new(),
            })
        }
        async fn execute(&self, _d: &str, _q: &str, _m: usize) -> DomainResult<QueryResult> {
            Ok(QueryResult {
                columns: vec![QueryColumn {
                    name: "x".into(),
                    data_type: "text".into(),
                }],
                rows: vec![datara_domain::QueryRow {
                    cells: vec![
                        Value::Text("dbo".into()),
                        Value::Text("users".into()),
                        Value::Text("table".into()),
                    ],
                }],
                rows_affected: None,
                truncated: false,
            })
        }
        async fn cancel(&self) -> DomainResult<()> {
            Ok(())
        }
        fn quote_ident(&self, ident: &str) -> String {
            format!("[{ident}]")
        }
    }

    /// `DatabaseService::new` needs `D: Default`.
    #[derive(Default)]
    struct StubDriver;

    #[async_trait]
    impl DatabaseDriver for StubDriver {
        async fn connect(
            &self,
            _p: &ConnectionProfile,
            _c: &Credentials,
        ) -> DomainResult<Box<dyn DatabaseSession>> {
            Ok(Box::new(StubSession))
        }
    }

    fn tool_text(result: &CallToolResult) -> String {
        match &result.content[0] {
            ContentBlock::Text(t) => t.text.clone(),
            _ => panic!("expected text content"),
        }
    }

    async fn mcp_with(
        max_rows: usize,
        allow_writes: bool,
    ) -> (DataraMcp<StubDriver>, Arc<Storage>, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let storage = Arc::new(Storage::open(&dir.path().join("test.db")).await.unwrap());
        storage
            .connections()
            .insert(NewConnection {
                name: "it".into(),
                host: "127.0.0.1".into(),
                port: 1433,
                database: Some("appdb".into()),
                username: "sa".into(),
                authentication: AuthenticationMode::SqlPassword,
                encryption: EncryptionMode::Preferred,
                trust_server_certificate: true,
            })
            .await
            .unwrap();
        let service = Arc::new(DatabaseService::<StubDriver>::new(
            Arc::clone(&storage),
            Arc::new(StubSecrets),
        ));
        (
            DataraMcp::new(service, Arc::clone(&storage), max_rows, allow_writes),
            storage,
            dir,
        )
    }

    async fn mcp() -> (DataraMcp<StubDriver>, Arc<Storage>, tempfile::TempDir) {
        mcp_with(1000, false).await
    }

    #[tokio::test]
    async fn list_connections_returns_profiles_without_secret_reference() {
        let (mcp, _s, _d) = mcp().await;
        let result = mcp.list_connections().await.unwrap();
        let json = tool_text(&result);
        let conns: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(conns["connections"][0]["name"], "it");
        assert_eq!(conns["connections"][0]["port"], 1433);
        assert_eq!(conns["truncated"], false);
        // The whole payload must not mention secrets at all.
        assert!(!json.contains("secret"));
    }

    #[tokio::test]
    async fn list_databases_uses_pooled_session() {
        let (mcp, _s, _d) = mcp().await;
        let result = mcp
            .list_databases(Parameters(ConnectionArgs { conn_id: 1 }))
            .await
            .unwrap();
        let json = tool_text(&result);
        assert!(json.contains("appdb"));
    }

    #[tokio::test]
    async fn list_tables_defaults_schema_and_profile_database() {
        let (mcp, _s, _d) = mcp().await;
        let result = mcp
            .list_tables(Parameters(TablesArgs {
                conn_id: 1,
                database: None,
                schema: None,
            }))
            .await
            .unwrap();
        let json = tool_text(&result);
        // Stub echoes the schema it was called with.
        assert!(json.contains(DEFAULT_SCHEMA));
        assert!(json.contains("users"));
    }

    #[tokio::test]
    async fn search_schema_maps_rows_and_escapes_quotes() {
        let (mcp, _s, _d) = mcp().await;
        let result = mcp
            .search_schema(Parameters(SearchArgs {
                conn_id: 1,
                query: "us'er".into(),
                database: None,
            }))
            .await
            .unwrap();
        let json = tool_text(&result);
        assert!(search_sql("us'er").contains("us''er"));
        assert!(search_sql("a[b").contains("a[[]b"));
        // Fixed stub row maps to a TableInfo under `tables`.
        assert!(json.contains("users"));
        assert!(json.contains("dbo"));
    }

    #[tokio::test]
    async fn row_tools_report_truncation() {
        // Stub returns 2 databases; cap at 1 → truncated flag must surface.
        let (mcp, _s, _d) = mcp_with(1, false).await;
        let result = mcp
            .list_databases(Parameters(ConnectionArgs { conn_id: 1 }))
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_str(&tool_text(&result)).unwrap();
        assert_eq!(json["databases"].as_array().unwrap().len(), 1);
        assert_eq!(json["truncated"], true);
    }

    #[tokio::test]
    async fn execute_query_returns_result_and_caps_rows() {
        let (mcp, _s, _d) = mcp().await;
        let result = mcp
            .execute_query(Parameters(QueryArgs {
                conn_id: 1,
                query: "SELECT 1".into(),
                database: None,
            }))
            .await
            .unwrap();
        let json = tool_text(&result);
        assert!(json.contains("dbo")); // stub rowset echoed through
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["statement_type"], "read");
        // §18: row_count is the emitted row count (post-cap).
        assert_eq!(parsed["row_count"], 1);

        let mut big = QueryResult {
            columns: vec![],
            rows: (0..5)
                .map(|i| datara_domain::QueryRow {
                    cells: vec![Value::Int(i)],
                })
                .collect(),
            rows_affected: None,
            truncated: false,
        };
        cap_rows(&mut big, 3);
        assert_eq!(big.rows.len(), 3);
        assert!(big.truncated);
    }

    #[tokio::test]
    async fn unknown_connection_is_tool_error_not_panic() {
        let (mcp, _s, _d) = mcp().await;
        let result = mcp
            .list_databases(Parameters(ConnectionArgs { conn_id: 999 }))
            .await
            .unwrap();
        assert_eq!(result.is_error, Some(true));
        assert!(tool_text(&result).contains("not found"));
    }

    #[tokio::test]
    async fn execute_query_refuses_writes_by_default() {
        let (mcp, _s, _d) = mcp().await;
        for query in [
            "DELETE FROM t",
            "UPDATE t SET a = 1",
            "DROP TABLE t",
            "SELECT * INTO t2 FROM t",
            // Table-function smuggling: OPENQUERY can run remote DML,
            // OPENROWSET embeds connection strings — both refused.
            "SELECT * FROM OPENQUERY(lk, 'DELETE FROM t')",
            "SELECT * FROM OPENROWSET('SQLOLEDB','srv';'sa';'pw','SELECT 1')",
            // CTE bodies are walked too.
            "WITH x AS (SELECT * INTO t2 FROM t) SELECT * FROM x",
        ] {
            let result = mcp
                .execute_query(Parameters(QueryArgs {
                    conn_id: 1,
                    query: query.into(),
                    database: None,
                }))
                .await
                .unwrap();
            assert_eq!(result.is_error, Some(true), "{query}");
            assert!(tool_text(&result).contains("mcp.allow_writes"), "{query}");
        }
        // SQL the parser can't read is refused too — never assume a read.
        let result = mcp
            .execute_query(Parameters(QueryArgs {
                conn_id: 1,
                query: "NOT VALID SQL ((".into(),
                database: None,
            }))
            .await
            .unwrap();
        assert_eq!(result.is_error, Some(true));
    }

    #[tokio::test]
    async fn execute_query_runs_writes_when_enabled() {
        let (mcp, _s, _d) = mcp_with(1000, true).await;
        let result = mcp
            .execute_query(Parameters(QueryArgs {
                conn_id: 1,
                query: "DELETE FROM t".into(),
                database: None,
            }))
            .await
            .unwrap();
        assert_eq!(result.is_error, Some(false));
        let json: serde_json::Value = serde_json::from_str(&tool_text(&result)).unwrap();
        assert_eq!(json["statement_type"], "write");
    }
}
