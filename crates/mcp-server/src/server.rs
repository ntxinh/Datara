//! The [`DataraMcp`] server: shared service + storage handles plus the
//! `#[tool_router]` macro glue that wires the `tools.rs` methods into
//! `tools/list` and `tools/call`.

use std::sync::Arc;

use datara_database::{DatabaseDriver, DatabaseService};
use datara_domain::{ConnectionId, QueryResult};
use datara_storage::Storage;
use rmcp::{
    handler::server::router::tool::ToolRouter,
    model::{Implementation, ServerCapabilities, ServerConfig},
    tool_handler, ServerHandler, ServiceExt,
};

/// MSSQL's default schema — used when a caller omits `schema`.
pub(crate) const DEFAULT_SCHEMA: &str = "dbo";
/// `execute`'s fallback when neither the call nor the profile names a
/// database (mirrors `DatabaseService::execute`).
pub(crate) const DEFAULT_DATABASE: &str = "master";

/// MCP server state. `service` is the same pooled `DatabaseService` type
/// the GUI uses; `storage` backs `list_connections` and database-name
/// fallbacks. `max_rows` caps rows any tool returns (config
/// `mcp.max_result_rows`).
pub struct DataraMcp<D: DatabaseDriver + 'static> {
    pub(crate) service: Arc<DatabaseService<D>>,
    pub(crate) storage: Arc<Storage>,
    pub(crate) max_rows: usize,
    pub(crate) tool_router: ToolRouter<Self>,
}

impl<D: DatabaseDriver + 'static> DataraMcp<D> {
    pub fn new(service: Arc<DatabaseService<D>>, storage: Arc<Storage>, max_rows: usize) -> Self {
        Self {
            service,
            storage,
            max_rows,
            tool_router: Self::tool_router(),
        }
    }

    /// Resolve an optional `database` argument: explicit value wins, then
    /// the profile's configured database, then `master`.
    pub(crate) async fn resolve_database(
        &self,
        conn_id: ConnectionId,
        database: Option<&str>,
    ) -> datara_domain::Result<String> {
        if let Some(db) = database {
            return Ok(db.to_owned());
        }
        Ok(self
            .storage
            .connections()
            .get(conn_id)
            .await?
            .database
            .unwrap_or_else(|| DEFAULT_DATABASE.to_owned()))
    }
}

// `#[derive(Clone)]` would demand `D: Clone`; every field is `Arc`/Copy.
impl<D: DatabaseDriver + 'static> Clone for DataraMcp<D> {
    fn clone(&self) -> Self {
        Self {
            service: Arc::clone(&self.service),
            storage: Arc::clone(&self.storage),
            max_rows: self.max_rows,
            tool_router: self.tool_router.clone(),
        }
    }
}

#[tool_handler]
impl<D: DatabaseDriver + 'static> ServerHandler for DataraMcp<D> {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::from_build_env())
            .with_instructions(
                "Datara: MSSQL inspection and queries. Tools: list_connections, \
             list_databases, list_tables, describe_table, search_schema, \
             execute_query. `conn_id` arguments come from list_connections. \
             Schema arguments default to 'dbo'; database arguments fall back \
             to the connection's configured database, then 'master'."
                    .to_string(),
            )
    }
}

/// Truncate `result` rows to `max`, setting the `truncated` flag when rows
/// were cut. The driver already caps materialization; this is the MCP-side
/// belt to the same suspenders.
pub(crate) fn cap_rows(result: &mut QueryResult, max: usize) {
    if result.rows.len() > max {
        result.rows.truncate(max);
        result.truncated = true;
    }
}

/// Serve over stdio until the client disconnects. Stdout is the transport —
/// all logging must go to stderr (see `mcp_main`).
pub async fn serve_stdio<D: DatabaseDriver + 'static>(
    service: Arc<DatabaseService<D>>,
    storage: Arc<Storage>,
    max_rows: usize,
) -> anyhow::Result<()> {
    let server = DataraMcp::new(service, storage, max_rows);
    let running = server.serve(rmcp::transport::stdio()).await?;
    running.waiting().await?;
    Ok(())
}
