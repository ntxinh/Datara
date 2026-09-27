mod bridge;
mod commands;
mod editor_ui;
mod schema_tree;
mod services;
mod ui;

use clap::{Parser, Subcommand};
use tracing_subscriber::EnvFilter;

slint::include_modules!();

#[derive(Parser)]
#[command(name = "datara", version, about = "Datara — MSSQL client")]
struct Cli {
    #[command(subcommand)]
    command: Option<Cmd>,
}

#[derive(Subcommand)]
enum Cmd {
    /// Launch the GUI (default).
    Gui,
    /// Serve MCP over stdio.
    McpServe,
}

fn main() -> anyhow::Result<()> {
    match Cli::parse().command.unwrap_or(Cmd::Gui) {
        Cmd::Gui => run_gui(),
        Cmd::McpServe => {
            eprintln!("mcp-serve: not yet implemented");
            std::process::exit(2);
        }
    }
}

fn run_gui() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("info,datara=debug")),
        )
        .init();

    // Prefer native Wayland over XWayland when running under a Wayland
    // session, unless the user picked a backend themselves.
    if std::env::var_os("WAYLAND_DISPLAY").is_some()
        && std::env::var_os("WINIT_UNIX_BACKEND").is_none()
    {
        std::env::set_var("WINIT_UNIX_BACKEND", "wayland");
    }

    // Install the winit backend eagerly so the platform context exists for
    // set_xdg_app_id (normally created lazily on first window).
    slint::BackendSelector::new().select()?;

    // Wayland app_id / X11 WM_CLASS — must be set before the window is shown
    // and must match StartupWMClass in the .desktop file (Task 8.1).
    slint::set_xdg_app_id("datara")?;

    let services = services::AppServices::init()?;
    ui::run(services)?;
    Ok(())
}
