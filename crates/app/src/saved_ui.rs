//! Saved queries (Task 6.2): `SavedRepo` → `SavedItem` model mapping, and
//! the `Backend` actions behind `Bridge.save-query` / `saved-*` — save the
//! active editor buffer under a name, list, open into a new tab, delete.
//!
//! Opening loads the text for editing — deliberately no auto-execute
//! (`history_rerun` stays the load-and-run path).

use crate::bridge::{AppEvent, UiHandle};
use crate::history_ui::preview_line;
use crate::services::Backend;
use crate::SavedItem;

/// `SavedQuery` → the Slint row model.
fn saved_item(q: &datara_domain::SavedQuery) -> SavedItem {
    SavedItem {
        id: q.id as i32,
        name: q.name.clone().into(),
        query: preview_line(&q.query).into(),
    }
}

/// Case-insensitive substring match on name or query text — empty filter
/// matches everything. List is bounded (one row per Ctrl+S), so an
/// in-memory filter is enough; no LIKE needed.
fn saved_matches(q: &datara_domain::SavedQuery, filter: &str) -> bool {
    let f = filter.to_lowercase();
    f.is_empty() || q.name.to_lowercase().contains(&f) || q.query.to_lowercase().contains(&f)
}

impl Backend {
    /// `Bridge.saved-search`: saved queries matching `filter` on name or
    /// query text, pushed to the panel's Saved tab.
    pub async fn saved_list(&self, filter: &str, ui: UiHandle) {
        match self.storage.saved().list().await {
            Ok(entries) => {
                let items = entries
                    .iter()
                    .filter(|q| saved_matches(q, filter))
                    .map(saved_item)
                    .collect();
                ui.dispatch(AppEvent::SavedLoaded(items));
            }
            Err(e) => ui.dispatch(AppEvent::Status(format!("Saved list failed: {e}"))),
        }
    }

    /// `Bridge.save-query`: persist the active tab's text under `name`,
    /// then refresh the Saved list so a new row shows without a reload.
    pub async fn save_query(&self, name: &str, query: &str, filter: &str, ui: UiHandle) {
        match self.storage.saved().save(name, query).await {
            Ok(_) => {
                ui.dispatch(AppEvent::Status("Saved".into()));
                self.saved_list(filter, ui).await;
            }
            Err(e) => ui.dispatch(AppEvent::Status(format!("Save failed: {e}"))),
        }
    }

    /// `Bridge.saved-open`: fetch the saved query and open it in a new
    /// editor tab — load only, no execute.
    pub async fn saved_open(&self, id: i64, ui: UiHandle) {
        match self.storage.saved().get(id).await {
            Ok(q) => ui.dispatch(AppEvent::OpenSavedSql {
                label: q.name,
                sql: q.query,
            }),
            Err(e) => ui.dispatch(AppEvent::Status(format!("Saved query failed: {e}"))),
        }
    }

    /// `Bridge.saved-delete`: delete the row, then refresh the list.
    pub async fn saved_delete(&self, id: i64, filter: &str, ui: UiHandle) {
        match self.storage.saved().delete(id).await {
            Ok(()) => self.saved_list(filter, ui).await,
            Err(e) => ui.dispatch(AppEvent::Status(format!("Saved delete failed: {e}"))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The Saved row shows name + a single-line preview; multi-line text
    /// is truncated to its first line (full text returns on Open).
    #[test]
    fn saved_item_previews_first_line() {
        let q = datara_domain::SavedQuery {
            id: 3,
            name: "n".into(),
            query: "select a\nselect b".into(),
            created_at: "2026-09-28T10:00:00Z".parse().unwrap(),
        };
        let i = saved_item(&q);
        assert_eq!(i.id, 3);
        assert_eq!(i.query.as_str(), "select a");
    }

    /// Filter matches name or query, case-insensitive; empty matches all.
    #[test]
    fn saved_matches_filters_name_and_query() {
        let q = datara_domain::SavedQuery {
            id: 1,
            name: "Daily report".into(),
            query: "select * from Orders".into(),
            created_at: "2026-09-28T10:00:00Z".parse().unwrap(),
        };
        assert!(saved_matches(&q, ""));
        assert!(saved_matches(&q, "daily"));
        assert!(saved_matches(&q, "ORDERS"));
        assert!(!saved_matches(&q, "nope"));
    }
}
