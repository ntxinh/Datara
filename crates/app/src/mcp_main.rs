//! `datara mcp-serve`: runs the MCP server over stdio. Stdout is the
//! protocol transport, so tracing goes to stderr and nothing else may
//! write to stdout.

use std::sync::Arc;

use datara_config::{AppConfig, AppPaths};
use datara_database::DatabaseService;
use datara_driver_mssql::MssqlDriver;
use datara_secrets::SecretStore;
use datara_storage::Storage;
use tracing_subscriber::EnvFilter;

/// Entry point for `Cmd::McpServe` — called inside the tokio runtime.
pub async fn run() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .init();

    let paths = AppPaths::new()?;
    let config = AppConfig::load(&paths)?;
    if !config.mcp.enabled {
        eprintln!(
            "datara mcp-serve: MCP is disabled — set mcp.enabled = true in {}",
            paths.config_file().display()
        );
        return Ok(());
    }

    let storage = Arc::new(Storage::open(&paths.app_db()).await?);
    let secrets = Arc::new(SecretStore::connect().await?);
    let service = Arc::new(DatabaseService::<MssqlDriver>::new(
        Arc::clone(&storage),
        Arc::clone(&secrets),
    ));

    tracing::info!(
        max_rows = config.mcp.max_result_rows,
        allow_writes = config.mcp.allow_writes,
        "serving MCP on stdio"
    );
    datara_mcp_server::serve_stdio(
        service,
        storage,
        config.mcp.max_result_rows as usize,
        config.mcp.allow_writes,
    )
    .await
}
