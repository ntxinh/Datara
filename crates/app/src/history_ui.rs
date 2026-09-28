//! Query-history panel (Task 6.1): `HistoryRepo` → `HistoryItem` model
//! mapping, and the `Backend` actions behind `Bridge.history-*` — search,
//! rerun (load into a new tab + execute), copy, delete.
//!
//! All methods run on the Tokio runtime and report through [`UiHandle`];
//! the panel itself is a bottom pane in `ui/pages/history.slint`.

use datara_domain::{ConnectionId, QueryHistoryEntry};

use crate::bridge::{AppEvent, UiHandle};
use crate::services::Backend;
use crate::HistoryItem;

/// Cap on rows pulled for the panel — newest-first.
const HISTORY_LIMIT: u32 = 200;

/// First line of `query` capped at 80 chars — the single-line preview the
/// history and saved panels show (full text comes back on open/rerun).
pub(crate) fn preview_line(query: &str) -> String {
    let first = query.lines().next().unwrap_or_default();
    match first.char_indices().nth(80) {
        Some((i, _)) => format!("{}…", &first[..i]),
        None => first.to_owned(),
    }
}

/// `QueryHistoryEntry` → the Slint row model.
fn history_item(e: &QueryHistoryEntry) -> HistoryItem {
    HistoryItem {
        id: e.id as i32,
        query: preview_line(&e.query).into(),
        started_at: local_timestamp(e.started_at).into(),
        duration: format_duration(e.duration_ms).into(),
        rows: format!("{} rows", e.row_count).into(),
        ok: e.success,
    }
}

/// `340ms` under a second, `1.2s` above.
fn format_duration(ms: u64) -> String {
    if ms >= 1000 {
        format!("{:.1}s", ms as f64 / 1000.0)
    } else {
        format!("{ms}ms")
    }
}

/// Stored RFC3339 → local `YYYY-MM-DD HH:MM` in the system zone.
fn local_timestamp(ts: jiff::Timestamp) -> String {
    ts.to_zoned(jiff::tz::TimeZone::system())
        .strftime("%Y-%m-%d %H:%M")
        .to_string()
}

impl Backend {
    /// `Bridge.history-search`: newest-first history rows for `conn_id`
    /// (the active tab's connection, `None` = all) matching `filter`.
    pub async fn history_search(&self, conn_id: Option<ConnectionId>, filter: &str, ui: UiHandle) {
        match self
            .storage
            .history()
            .search(conn_id, filter, HISTORY_LIMIT)
            .await
        {
            Ok(entries) => {
                let items: Vec<HistoryItem> = entries.iter().map(history_item).collect();
                ui.dispatch(AppEvent::HistoryLoaded(items));
            }
            Err(e) => ui.dispatch(AppEvent::Status(format!("History search failed: {e}"))),
        }
    }

    /// `Bridge.history-rerun`: fetch the entry, open a tab with its query
    /// and execute — the same load-and-run path table previews take.
    pub async fn history_rerun(&self, id: i64, ui: UiHandle) {
        match self.storage.history().get(id).await {
            Ok(Some(e)) => ui.dispatch(AppEvent::PreviewSql {
                conn_id: e.connection_id,
                database: e.database,
                label: "Rerun".into(),
                sql: e.query,
            }),
            Ok(None) => ui.dispatch(AppEvent::Status("History entry no longer exists".into())),
            Err(e) => ui.dispatch(AppEvent::Status(format!("History lookup failed: {e}"))),
        }
    }

    /// `Bridge.history-copy`: copy the entry's full query text. Clipboard
    /// work happens on the UI thread in `apply` — Wayland's arboard owner
    /// must outlive the call, so no Clipboard crosses this boundary.
    pub async fn history_copy(&self, id: i64, ui: UiHandle) {
        match self.storage.history().get(id).await {
            Ok(Some(e)) => ui.dispatch(AppEvent::CopyHistoryText(e.query)),
            Ok(None) => ui.dispatch(AppEvent::Status("History entry no longer exists".into())),
            Err(e) => ui.dispatch(AppEvent::Status(format!("History lookup failed: {e}"))),
        }
    }

    /// `Bridge.history-delete`: delete the row, then refresh the list with
    /// the caller's current scope/filter.
    pub async fn history_delete(
        &self,
        id: i64,
        conn_id: Option<ConnectionId>,
        filter: &str,
        ui: UiHandle,
    ) {
        match self.storage.history().delete(id).await {
            Ok(()) => self.history_search(conn_id, filter, ui).await,
            Err(e) => ui.dispatch(AppEvent::Status(format!("History delete failed: {e}"))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(query: &str, ms: u64, success: bool) -> QueryHistoryEntry {
        QueryHistoryEntry {
            id: 42,
            connection_id: ConnectionId(7),
            database: Some("appdb".into()),
            query: query.into(),
            started_at: "2026-09-27T10:00:00Z".parse().unwrap(),
            duration_ms: ms,
            row_count: 7,
            success,
            error_message: None,
        }
    }

    /// Duration renders "1.2s"/"340ms"; the timestamp lands in local
    /// `YYYY-MM-DD HH:MM` regardless of the host zone.
    #[test]
    fn history_item_formats_duration_and_timestamp() {
        let i = history_item(&entry("select 1", 340, true));
        assert_eq!(i.duration.as_str(), "340ms");
        let i = history_item(&entry("select 1", 1200, true));
        assert_eq!(i.duration.as_str(), "1.2s");
        let ts = i.started_at.as_str();
        assert_eq!(ts.len(), 16, "YYYY-MM-DD HH:MM, got {ts:?}");
        assert_eq!(&ts[4..5], "-");
        assert_eq!(&ts[13..14], ":");
    }

    /// Only the first line is shown, capped at 80 chars with "…".
    #[test]
    fn history_item_truncates_query_to_first_80_chars() {
        let long = format!("{}\nselect b", "x".repeat(200));
        let i = history_item(&entry(&long, 5, false));
        assert_eq!(i.query.chars().count(), 81);
        assert!(i.query.ends_with('…'));
        assert!(!i.ok);

        let i = history_item(&entry("select a\nselect b", 5, true));
        assert_eq!(i.query.as_str(), "select a");
    }

    /// Repo round-trip for `get` — the path rerun/copy/delete depend on.
    #[tokio::test]
    async fn history_repo_get_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let storage = datara_storage::Storage::open(&dir.path().join("t.db"))
            .await
            .unwrap();
        let conn = storage
            .connections()
            .insert(datara_storage::NewConnection {
                name: "h".into(),
                host: "db.example.com".into(),
                port: 1433,
                database: Some("appdb".into()),
                username: "sa".into(),
                authentication: datara_domain::AuthenticationMode::SqlPassword,
                encryption: datara_domain::EncryptionMode::Required,
                trust_server_certificate: true,
            })
            .await
            .unwrap();
        let repo = storage.history();
        let mut e = entry("select full query", 9, true);
        e.connection_id = conn;
        let id = repo.record(&e).await.unwrap();

        let e = repo.get(id).await.unwrap().expect("row exists");
        assert_eq!(e.query, "select full query");
        assert_eq!(e.connection_id, conn);

        repo.delete(id).await.unwrap();
        assert!(repo.get(id).await.unwrap().is_none());
    }
}
