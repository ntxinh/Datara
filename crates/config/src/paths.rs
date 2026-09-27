//! XDG application paths, with `DATARA_*_DIR` env overrides for tests and Flatpak.

use std::path::{Path, PathBuf};

use datara_domain::{DomainError, Result};

/// Application identifier appended under each XDG base directory.
const APP_ID: &str = "io.github.ntxinh.Datara";

/// Resolved per-app directories under the XDG base dirs.
pub struct AppPaths {
    data_dir: PathBuf,
    config_dir: PathBuf,
    state_dir: PathBuf,
}

impl AppPaths {
    /// Resolves base dirs (`DATARA_DATA_DIR`/`DATARA_CONFIG_DIR`/`DATARA_STATE_DIR`
    /// override, else `dirs` + app id) and creates them.
    pub fn new() -> Result<Self> {
        let paths = Self {
            data_dir: resolve("DATARA_DATA_DIR", dirs::data_dir)?,
            config_dir: resolve("DATARA_CONFIG_DIR", dirs::config_dir)?,
            state_dir: resolve("DATARA_STATE_DIR", dirs::state_dir)?,
        };
        for dir in [&paths.data_dir, &paths.config_dir, &paths.log_dir()] {
            create(dir)?;
        }
        Ok(paths)
    }

    /// `data_dir/app.db` — the sqlx metadata database.
    pub fn app_db(&self) -> PathBuf {
        self.data_dir.join("app.db")
    }

    /// `config_dir/config.toml` — user settings file.
    pub fn config_file(&self) -> PathBuf {
        self.config_dir.join("config.toml")
    }

    /// `state_dir/logs` — log output directory.
    pub fn log_dir(&self) -> PathBuf {
        self.state_dir.join("logs")
    }

    /// Constructs paths under explicit dirs (test-only; bypasses env/XDG).
    #[cfg(test)]
    pub(crate) fn new_for_test(data_dir: PathBuf, config_dir: PathBuf, state_dir: PathBuf) -> Self {
        Self {
            data_dir,
            config_dir,
            state_dir,
        }
    }
}

/// Env var wins verbatim (non-empty); otherwise `fallback() + APP_ID`.
fn resolve(env_var: &str, fallback: impl Fn() -> Option<PathBuf>) -> Result<PathBuf> {
    if let Some(dir) = std::env::var_os(env_var).filter(|v| !v.is_empty()) {
        return Ok(PathBuf::from(dir));
    }
    fallback()
        .map(|base| base.join(APP_ID))
        .ok_or_else(|| DomainError::Config {
            message: format!("cannot determine base directory for {env_var}"),
        })
}

fn create(dir: &Path) -> Result<()> {
    std::fs::create_dir_all(dir).map_err(|e| DomainError::Config {
        message: format!("cannot create directory {}: {e}", dir.display()),
    })
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use tempfile::TempDir;

    use super::*;

    #[rstest]
    fn derived_files_live_under_base_dirs() {
        let dir = TempDir::new().unwrap();
        let paths = AppPaths::new_for_test(
            dir.path().join("data"),
            dir.path().join("config"),
            dir.path().join("state"),
        );
        assert_eq!(paths.app_db(), dir.path().join("data/app.db"));
        assert_eq!(paths.config_file(), dir.path().join("config/config.toml"));
        assert_eq!(paths.log_dir(), dir.path().join("state/logs"));
    }

    #[rstest]
    fn env_vars_override_xdg_dirs_and_dirs_are_created() {
        let dir = TempDir::new().unwrap();
        let data = dir.path().join("d");
        let config = dir.path().join("c");
        let state = dir.path().join("s");
        // Single test touches env to avoid races between parallel tests.
        std::env::set_var("DATARA_DATA_DIR", &data);
        std::env::set_var("DATARA_CONFIG_DIR", &config);
        std::env::set_var("DATARA_STATE_DIR", &state);

        let paths = AppPaths::new().unwrap();

        std::env::remove_var("DATARA_DATA_DIR");
        std::env::remove_var("DATARA_CONFIG_DIR");
        std::env::remove_var("DATARA_STATE_DIR");

        assert_eq!(paths.app_db(), data.join("app.db"));
        assert_eq!(paths.config_file(), config.join("config.toml"));
        assert_eq!(paths.log_dir(), state.join("logs"));
        assert!(data.is_dir() && config.is_dir() && state.join("logs").is_dir());
    }

    #[rstest]
    fn xdg_base_gets_app_id_suffix() {
        let base = PathBuf::from("/tmp/example");
        let resolved = resolve("DATARA_UNSET_VAR_FOR_TEST", || Some(base.clone())).unwrap();
        assert_eq!(resolved, base.join(APP_ID));
    }

    #[rstest]
    fn unresolvable_base_maps_to_config_error() {
        let err = resolve("DATARA_UNSET_VAR_FOR_TEST", || None).unwrap_err();
        assert!(matches!(err, DomainError::Config { .. }));
    }

    #[rstest]
    fn create_failure_maps_to_config_error() {
        let err = create(Path::new("/proc/1/no-such-parent/leaf")).unwrap_err();
        assert!(matches!(err, DomainError::Config { .. }));
    }
}
