use std::path::Path;

use datara_domain::Result;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::SqlitePool;

use crate::storage_err;

/// SQLite-backed store for profiles, query history, and saved queries.
pub struct Storage {
    pool: SqlitePool,
}

impl Storage {
    /// Open (creating if needed) the database at `path` and run migrations.
    /// Parent directories are created as needed.
    pub async fn open(path: &Path) -> Result<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(storage_err)?;
        }
        let options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true);
        let pool = SqlitePoolOptions::new()
            .connect_with(options)
            .await
            .map_err(storage_err)?;
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .map_err(storage_err)?;
        Ok(Self { pool })
    }

    /// Repository for connection profiles.
    pub fn connections(&self) -> crate::ConnectionRepo<'_> {
        crate::ConnectionRepo::new(self)
    }

    /// Repository for query history.
    pub fn history(&self) -> crate::HistoryRepo<'_> {
        crate::HistoryRepo::new(self)
    }

    pub(crate) fn pool(&self) -> &SqlitePool {
        &self.pool
    }
}
