use datara_domain::{DomainError, Result, SavedQuery};
use sqlx::FromRow;

use crate::{storage_err, Storage};

/// Repository over the `saved_queries` table.
pub struct SavedRepo<'a> {
    storage: &'a Storage,
}

impl<'a> SavedRepo<'a> {
    pub(crate) fn new(storage: &'a Storage) -> Self {
        Self { storage }
    }

    /// Save `query` under `name`; returns the assigned row id.
    /// `created_at` is stored as RFC3339 TEXT like `query_history`.
    pub async fn save(&self, name: &str, query: &str) -> Result<i64> {
        let result =
            sqlx::query("INSERT INTO saved_queries (name, query, created_at) VALUES (?, ?, ?)")
                .bind(name)
                .bind(query)
                .bind(jiff::Timestamp::now().to_string())
                .execute(self.storage.pool())
                .await
                .map_err(storage_err)?;
        Ok(result.last_insert_rowid())
    }

    /// All saved queries, ordered by name.
    pub async fn list(&self) -> Result<Vec<SavedQuery>> {
        let rows = sqlx::query_as::<_, SavedRow>(
            "SELECT id, name, query, created_at FROM saved_queries \
             ORDER BY name COLLATE NOCASE, id",
        )
        .fetch_all(self.storage.pool())
        .await
        .map_err(storage_err)?;
        rows.into_iter().map(SavedRow::into_query).collect()
    }

    /// Fetch one saved query by id — error when it doesn't exist.
    pub async fn get(&self, id: i64) -> Result<SavedQuery> {
        let row = sqlx::query_as::<_, SavedRow>(
            "SELECT id, name, query, created_at FROM saved_queries WHERE id = ?",
        )
        .bind(id)
        .fetch_one(self.storage.pool())
        .await
        .map_err(|e| match e {
            sqlx::Error::RowNotFound => DomainError::Storage {
                message: format!("saved query {id} not found"),
            },
            other => storage_err(other),
        })?;
        row.into_query()
    }

    /// Delete one saved query by id.
    pub async fn delete(&self, id: i64) -> Result<()> {
        sqlx::query("DELETE FROM saved_queries WHERE id = ?")
            .bind(id)
            .execute(self.storage.pool())
            .await
            .map_err(storage_err)?;
        Ok(())
    }
}

#[derive(FromRow)]
struct SavedRow {
    id: i64,
    name: String,
    query: String,
    created_at: String,
}

impl SavedRow {
    fn into_query(self) -> Result<SavedQuery> {
        Ok(SavedQuery {
            id: self.id,
            name: self.name,
            query: self.query,
            created_at: self.created_at.parse().map_err(storage_err)?,
        })
    }
}
