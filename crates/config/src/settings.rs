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
    /// Whether `execute_query` accepts non-SELECT statements. Default
    /// `false`: only read queries run; anything the parser can't prove is a
    /// read is refused (spec §17).
    pub allow_writes: bool,
}

impl Default for McpConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            max_result_rows: 1000,
            allow_writes: false,
        }
    }
}

/// Persisted UI layout: written back into `[workspace]` on window close,
/// applied at startup. `open_tabs` holds each editor tab's SQL text so
/// scratch queries survive a restart.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct WorkspaceState {
    /// Sidebar width in px.
    pub sidebar_width: u32,
    /// Editor pane's share of the editor/result split (0.0–1.0).
    pub editor_split: f32,
    /// Editor tab contents, in order.
    pub open_tabs: Vec<String>,
}

impl Default for WorkspaceState {
    fn default() -> Self {
        Self {
            sidebar_width: 240,
            editor_split: 0.5,
            open_tabs: Vec::new(),
        }
    }
}

/// Root application configuration, deserialized from `config.toml`.
#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct AppConfig {
    pub editor: EditorConfig,
    pub query: QueryConfig,
    pub appearance: AppearanceConfig,
    pub mcp: McpConfig,
    pub workspace: WorkspaceState,
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

    /// Writes the full config back to `paths.config_file()` — used by the
    /// GUI to persist `[workspace]` on window close.
    /// ponytail: whole-file rewrite drops comments/unknown keys; switch to
    /// toml_edit if users start hand-editing between runs.
    pub fn save(&self, paths: &AppPaths) -> Result<()> {
        let file = paths.config_file();
        let text = toml::to_string(self).map_err(|e| DomainError::Config {
            message: format!("cannot serialize config: {e}"),
        })?;
        std::fs::write(&file, text).map_err(|e| DomainError::Config {
            message: format!("cannot write {}: {e}", file.display()),
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
        assert!(!cfg.mcp.allow_writes);
        assert_eq!(cfg.workspace.sidebar_width, 240);
        assert_eq!(cfg.workspace.editor_split, 0.5);
        assert!(cfg.workspace.open_tabs.is_empty());
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

    /// Config audit: every serialized key must have a registered consumer.
    /// A new field fails here until it's wired into the app and added to
    /// this table.
    #[rstest]
    fn every_config_key_has_a_consumer() {
        const CONSUMED: &[&str] = &[
            "editor.font_size",        // ui.rs → Bridge.editor-font-size → TextInput font-size
            "editor.tab_size",         // ui.rs on_insert_tab → spaces at caret
            "query.default_limit",     // UiCtx::query_limit → preview TOP n + execute max_rows
            "query.timeout_seconds", // Backend::query_timeout → tokio::time::timeout → abort (cancel)
            "appearance.theme",      // ui.rs → Theme.dark
            "mcp.enabled",           // mcp_main.rs gate
            "mcp.max_result_rows",   // mcp_main.rs → serve_stdio max_rows
            "mcp.allow_writes",      // mcp_main.rs → serve_stdio allow_writes
            "workspace.sidebar_width", // ui.rs → Bridge.sidebar-width ↔ sidebar drag
            "workspace.editor_split", // ui.rs → Bridge.editor-split ↔ editor/grid split
            "workspace.open_tabs",   // ui.rs → EditorState::restore, written on close
        ];
        let cfg = toml::to_string(&AppConfig::default()).unwrap();
        let mut keys: Vec<String> = Vec::new();
        let mut section = String::new();
        for line in cfg.lines() {
            let line = line.trim();
            if let Some(name) = line.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
                section = name.to_string();
            } else if let Some((key, _)) = line.split_once('=') {
                keys.push(format!("{}.{}", section, key.trim()));
            }
        }
        for key in &keys {
            assert!(
                CONSUMED.contains(&key.as_str()),
                "config key `{key}` has no registered consumer — wire it or remove it"
            );
        }
        assert_eq!(keys.len(), CONSUMED.len(), "consumer table lists dead keys");
    }

    #[rstest]
    fn save_writes_workspace_state_back() {
        let dir = TempDir::new().unwrap();
        let paths = paths_in(&dir);
        fs::create_dir_all(paths.config_file().parent().unwrap()).unwrap();
        let mut cfg = AppConfig::default();
        cfg.workspace.sidebar_width = 300;
        cfg.workspace.open_tabs = vec!["select 1".into()];
        cfg.save(&paths).unwrap();

        let back = AppConfig::load(&paths).unwrap();
        assert_eq!(back.workspace.sidebar_width, 300);
        assert_eq!(back.workspace.open_tabs, ["select 1"]);
        // User settings ride along untouched.
        assert_eq!(back.editor.font_size, 14);
    }
}
