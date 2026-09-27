//! Datara configuration: XDG application paths and TOML settings.

mod paths;
mod settings;

pub use paths::AppPaths;
pub use settings::{AppConfig, AppearanceConfig, EditorConfig, McpConfig, QueryConfig};
