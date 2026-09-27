//! TOML application settings with serde defaults.

use datara_domain::{DomainError, Result};
use serde::{Deserialize, Serialize};

use crate::AppPaths;

/// Editor behavior settings.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default)]
pub struct EditorConfig {
    /// Editor font size in points.
    pub font_size: u16,
    /// Spaces per tab.
    pub tab_size: u8,
}

impl Default for EditorConfig {
    fn default() -> Self {
        Self {
            font_size: 14,
            tab_size: 4,
        }
    }
}

/// Query execution settings.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default)]
pub struct QueryConfig {
    /// Row limit applied when a query has no explicit TOP/limit.
    pub default_limit: u32,
    /// Query timeout in seconds.
    pub timeout_seconds: u64,
}

impl Default for QueryConfig {
    fn default() -> Self {
        Self {
            default_limit: 1000,
            timeout_seconds: 30,
        }
    }
}

/// UI appearance settings.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default)]
pub struct AppearanceConfig {
    /// Theme name: "system", "light", or "dark".
    pub theme: String,
}

impl Default for AppearanceConfig {
    fn default() -> Self {
        Self {
            theme: "system".into(),
        }
    }
}

/// Embedded MCP server settings.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default)]
pub struct McpConfig {
    /// Whether the MCP server starts with the app.
    pub enabled: bool,
    /// Maximum rows a single MCP tool call may return.
    pub max_result_rows: u32,
}

impl Default for McpConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            max_result_rows: 1000,
        }
    }
}

/// Root application configuration, deserialized from `config.toml`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default)]
pub struct AppConfig {
    pub editor: EditorConfig,
    pub query: QueryConfig,
    pub appearance: AppearanceConfig,
    pub mcp: McpConfig,
}

impl AppConfig {
    /// Default SQL Server port.
    pub fn default_port() -> u16 {
        1433
    }

    /// Loads `paths.config_file()`. A missing file yields defaults; malformed
    /// TOML yields `DomainError::Config` naming the file.
    pub fn load(paths: &AppPaths) -> Result<Self> {
        let file = paths.config_file();
        let exists = file.try_exists().map_err(|e| DomainError::Config {
            message: format!("cannot stat {}: {e}", file.display()),
        })?;
        if !exists {
            return Ok(Self::default());
        }
        let text = std::fs::read_to_string(&file).map_err(|e| DomainError::Config {
            message: format!("cannot read {}: {e}", file.display()),
        })?;
        toml::from_str(&text).map_err(|e| DomainError::Config {
            message: format!("invalid TOML in {}: {e}", file.display()),
        })
    }
}
#[cfg(test)]
mod tests {
    use std::fs;

    use datara_domain::DomainError;
    use rstest::rstest;
    use tempfile::TempDir;

    use super::*;
    use crate::AppPaths;

    fn paths_in(dir: &TempDir) -> AppPaths {
        AppPaths::new_for_test(
            dir.path().join("data"),
            dir.path().join("config"),
            dir.path().join("state"),
        )
    }

    fn write_config(paths: &AppPaths, toml_text: &str) {
        let file = paths.config_file();
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, toml_text).unwrap();
    }

    #[rstest]
    fn defaults_match_spec() {
        let cfg = AppConfig::default();
        assert_eq!(cfg.editor.font_size, 14);
        assert_eq!(cfg.editor.tab_size, 4);
        assert_eq!(cfg.query.default_limit, 1000);
        assert_eq!(cfg.query.timeout_seconds, 30);
        assert_eq!(cfg.appearance.theme, "system");
        assert!(!cfg.mcp.enabled);
        assert_eq!(cfg.mcp.max_result_rows, 1000);
        assert_eq!(AppConfig::default_port(), 1433);
    }

    #[rstest]
    fn defaults_round_trip_through_toml() {
        let cfg = AppConfig::default();
        let text = toml::to_string(&cfg).unwrap();
        let back: AppConfig = toml::from_str(&text).unwrap();
        assert_eq!(back, cfg);
    }

    #[rstest]
    fn missing_file_yields_defaults() {
        let dir = TempDir::new().unwrap();
        let cfg = AppConfig::load(&paths_in(&dir)).unwrap();
        assert_eq!(cfg, AppConfig::default());
    }

    #[rstest]
    fn partial_toml_keeps_unspecified_defaults() {
        let dir = TempDir::new().unwrap();
        let paths = paths_in(&dir);
        write_config(&paths, "[editor]\nfont_size = 18\n[mcp]\nenabled = true\n");

        let cfg = AppConfig::load(&paths).unwrap();
        assert_eq!(cfg.editor.font_size, 18);
        assert_eq!(cfg.editor.tab_size, 4);
        assert!(cfg.mcp.enabled);
        assert_eq!(cfg.mcp.max_result_rows, 1000);
        assert_eq!(cfg.query.default_limit, 1000);
        assert_eq!(cfg.appearance.theme, "system");
    }

    #[rstest]
    fn malformed_toml_errors_and_names_file() {
        let dir = TempDir::new().unwrap();
        let paths = paths_in(&dir);
        write_config(&paths, "this is [not toml");

        let err = AppConfig::load(&paths).unwrap_err();
        let msg = err.to_string();
        assert!(
            matches!(err, DomainError::Config { .. }),
            "expected Config error, got {msg}"
        );
        assert!(
            msg.contains(&paths.config_file().display().to_string()),
            "error should name the file: {msg}"
        );
    }
}
