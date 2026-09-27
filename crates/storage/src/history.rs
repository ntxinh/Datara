use datara_domain::{ConnectionId, QueryHistoryEntry, Result};
use sqlx::FromRow;

use crate::{storage_err, Storage};

/// Repository over the `query_history` table.
pub struct HistoryRepo<'a> {
    storage: &'a Storage,
}

impl<'a> HistoryRepo<'a> {
    pub(crate) fn new(storage: &'a Storage) -> Self {
        Self { storage }
    }

    /// Record one executed query; returns the assigned row id.
    /// `started_at` is stored as RFC3339 TEXT.
    pub async fn record(&self, entry: &QueryHistoryEntry) -> Result<i64> {
        let result = sqlx::query(
            "INSERT INTO query_history (connection_id, database, query, started_at, \
             duration_ms, row_count, success, error_message) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(entry.connection_id.0)
        .bind(&entry.database)
        .bind(&entry.query)
        .bind(entry.started_at.to_string())
        .bind(i64::try_from(entry.duration_ms).map_err(storage_err)?)
        .bind(i64::try_from(entry.row_count).map_err(storage_err)?)
        .bind(entry.success)
        .bind(&entry.error_message)
        .execute(self.storage.pool())
        .await
        .map_err(storage_err)?;
        Ok(result.last_insert_rowid())
    }

    /// Search history newest-first. `connection_id` scopes the search;
    /// `filter` is a substring (LIKE) match on the query text.
    pub async fn search(
        &self,
        connection_id: Option<ConnectionId>,
        filter: &str,
        limit: u32,
    ) -> Result<Vec<QueryHistoryEntry>> {
        let rows = sqlx::query_as::<_, HistoryRow>(
            "SELECT id, connection_id, database, query, started_at, duration_ms, \
             row_count, success, error_message FROM query_history \
             WHERE (?1 IS NULL OR connection_id = ?1) \
             AND query LIKE '%' || ?2 || '%' \
             ORDER BY started_at DESC LIMIT ?3",
        )
        .bind(connection_id.map(|id| id.0))
        .bind(filter)
        .bind(i64::from(limit))
        .fetch_all(self.storage.pool())
        .await
        .map_err(storage_err)?;
        rows.into_iter().map(HistoryRow::into_entry).collect()
    }

    /// Delete one history row by id.
    pub async fn delete(&self, id: i64) -> Result<()> {
        sqlx::query("DELETE FROM query_history WHERE id = ?")
            .bind(id)
            .execute(self.storage.pool())
            .await
            .map_err(storage_err)?;
        Ok(())
    }
}

#[derive(FromRow)]
struct HistoryRow {
    id: i64,
    connection_id: i64,
    database: Option<String>,
    query: String,
    started_at: String,
    duration_ms: i64,
    row_count: i64,
    success: bool,
    error_message: Option<String>,
}

impl HistoryRow {
    fn into_entry(self) -> Result<QueryHistoryEntry> {
        Ok(QueryHistoryEntry {
            id: self.id,
            connection_id: ConnectionId(self.connection_id),
            database: self.database,
            query: self.query,
            started_at: self.started_at.parse().map_err(storage_err)?,
            duration_ms: u64::try_from(self.duration_ms).map_err(storage_err)?,
            row_count: u64::try_from(self.row_count).map_err(storage_err)?,
            success: self.success,
            error_message: self.error_message,
        })
    }
}
