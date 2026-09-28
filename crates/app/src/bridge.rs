//! Slint ↔ Tokio boundary. Backend tasks spawn on the Tokio runtime and
//! report back through [`UiHandle::dispatch`], which hops onto the Slint
//! event loop (`invoke_from_event_loop`) before touching the window.
//!
//! The sidebar's [`SchemaTree`] lives behind `Arc<Mutex<_>>` shared between
//! `UiHandle` and the UI callbacks — mutex rather than `Rc<RefCell>` because
//! `UiHandle` must stay `Send` for `dispatch` (Tokio tasks hold it). Only
//! the UI thread ever takes the lock; Tokio tasks ship results back as
//! [`AppEvent`]s.

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::Mutex;

use crate::commands::{fuzzy, PALETTE_COMMANDS};
use crate::editor_ui::{highlight_spans, line_count, resolve_sql, EditorState};
use crate::grid::GridState;
use crate::schema_tree::{NodeKind, SchemaTree, TreeNode};
use crate::services::{preview_sql, Backend};
use crate::{
    Bridge, CommandItem, HighlightSpan, HistoryItem, MainWindow, SavedItem, TabItem,
    TreeNode as SlintTreeNode,
};
use datara_domain::{Command, ConnectionId, ConnectionProfile, QueryResult};
use slint::{ComponentHandle, ModelRc, VecModel, Weak};

/// Events flowing core → UI.
pub enum AppEvent {
    /// Full connection list refresh (initial load, after save/delete).
    ConnectionsLoaded(Vec<ConnectionProfile>),
    /// Free-form status line shown in the status bar.
    Status(String),
    /// Dialog Test button finished: `Ok(())` or the error's `Display`.
    ConnectTestResult(Result<(), String>),
    /// `toggle-node` fetch finished; splices under the expanded parent.
    TreeChildren {
        parent_id: i32,
        result: Result<Vec<TreeNode>, String>,
    },
    /// A table preview or history rerun: open a tab carrying `sql` bound
    /// to `conn_id`/`database`, then execute it.
    PreviewSql {
        conn_id: ConnectionId,
        database: Option<String>,
        sql: String,
        label: String,
    },
    /// A keyboard command (mpsc → event loop → here).
    Command(Command),
    /// `service.execute` returned a handle — the tab is running.
    QueryStarted {
        /// Editor tab that started running.
        tab: i32,
    },
    /// Query finished with a result — routed to `tab`'s slot
    /// ([`UiCtx::results`]), shown only if that tab is active.
    QueryResult {
        /// Editor tab that produced the result.
        tab: i32,
        result: QueryResult,
        elapsed_ms: u64,
    },
    /// Query failed or timed out; `message` is user-readable.
    QueryError {
        /// Editor tab the error belongs to.
        tab: i32,
        message: String,
    },
    /// The user cancelled the query (Stop button / `cancel-query`).
    QueryCancelled {
        /// Editor tab whose query was cancelled.
        tab: i32,
    },
    /// `history-search`/`history-delete` finished — swap the panel model.
    HistoryLoaded(Vec<HistoryItem>),
    /// `history-copy` resolved the entry — copy its full query text on the
    /// UI thread (clipboard lives in `UiCtx`).
    CopyHistoryText(String),
    /// `saved-search`/`save-query`/`saved-delete` finished — swap the
    /// Saved tab's model (Task 6.2).
    SavedLoaded(Vec<SavedItem>),
    /// `saved-open` resolved the query — load it into a new editor tab.
    /// Unlike `PreviewSql` this does NOT execute: saved queries open for
    /// editing.
    OpenSavedSql { label: String, sql: String },
}

/// Everything `apply` needs on the UI thread. Shared between `UiHandle`
/// (Tokio side) and the UI callbacks; mutexes rather than `RefCell` because
/// `UiHandle` must stay `Send` for `dispatch`.
pub struct UiCtx {
    pub tree: Mutex<SchemaTree>,
    /// Result grid state for the visible tab (Task 5.2): RowCache, column
    /// widths, selection.
    pub grid: Mutex<GridState>,
    /// Last query outcome per editor tab. Terminal `QueryResult`/`QueryError`/
    /// `QueryCancelled` events stash here keyed by tab id — a query finishing
    /// while the user views another tab is kept for when they switch back
    /// instead of overwriting the visible grid.
    pub results: Mutex<HashMap<i32, TabResult>>,
    pub editor: Mutex<EditorState>,
    pub backend: Arc<Backend>,
    /// Runtime handle to spawn backend work from the UI thread.
    pub handle: tokio::runtime::Handle,
    /// `[query] default_limit` from config.
    pub query_limit: u32,
    /// Shared clipboard — lazily created; on Wayland the `Clipboard` owns
    /// the data-control object that keeps copied text alive, so it must
    /// live as long as the app (used by the grid and history copy).
    pub clipboard: Mutex<Option<arboard::Clipboard>>,
}

/// What one editor tab's grid shows — the slot [`UiCtx::results`] stashes
/// per tab and [`show_tab_result`] restores on tab switch.
pub enum TabResult {
    /// Query in flight — the grid shows "Running…".
    Running,
    /// Finished: the domain result plus its `result-info` footer text.
    Rows(QueryResult, String),
    /// Failed: the grid footer shows `Error: <message>`.
    Failed(String),
    /// Stopped by the user (or timed out after a cancel).
    Cancelled,
}

/// Weak handle to the window, safe to move into Tokio tasks.
#[derive(Clone)]
pub struct UiHandle {
    weak: Weak<MainWindow>,
    cx: Arc<UiCtx>,
}

impl UiHandle {
    pub fn new(window: &MainWindow, cx: Arc<UiCtx>) -> Self {
        Self {
            weak: window.as_weak(),
            cx,
        }
    }

    /// A handle that dispatches into the void — for unit tests that drive
    /// backend methods without a window.
    #[cfg(test)]
    pub(crate) fn detached(cx: Arc<UiCtx>) -> Self {
        Self {
            weak: Weak::default(),
            cx,
        }
    }

    /// Post `event` onto the Slint event loop; no-op if the window is gone.
    pub fn dispatch(&self, event: AppEvent) {
        let weak = self.weak.clone();
        let cx = Arc::clone(&self.cx);
        let _ = slint::invoke_from_event_loop(move || {
            let Some(window) = weak.upgrade() else {
                return;
            };
            apply(&window, &cx, event);
        });
    }
}

pub(crate) fn apply(window: &MainWindow, cx: &Arc<UiCtx>, event: AppEvent) {
    let bridge = window.global::<Bridge>();
    match event {
        AppEvent::ConnectionsLoaded(profiles) => {
            let mut tree = cx.tree.lock();
            tree.set_connections(profiles);
            push_tree(&bridge, &tree);
        }
        AppEvent::Status(s) => bridge.set_status(s.into()),
        AppEvent::ConnectTestResult(Ok(())) => {
            bridge.set_test_result("Connection OK".into());
        }
        AppEvent::ConnectTestResult(Err(e)) => {
            bridge.set_test_result(format!("Failed: {e}").into());
        }
        AppEvent::TreeChildren { parent_id, result } => {
            let mut tree = cx.tree.lock();
            match result {
                Ok(children) => tree.replace_children(parent_id, children),
                // Collapse clears the Loading row; the status line carries
                // the actual error. Re-expand retries the fetch.
                Err(e) => {
                    if let Some(idx) = tree.find(parent_id) {
                        tree.collapse(idx);
                    }
                    bridge.set_status(format!("Schema load failed: {e}").into());
                }
            }
            push_tree(&bridge, &tree);
        }
        AppEvent::PreviewSql {
            conn_id,
            database,
            sql,
            label,
        } => {
            cx.backend.set_last_conn_id(conn_id);
            // Editor lock scoped to the tab+text setup so the execute below
            // (which re-locks via run_command) can't recurse on it.
            {
                let mut editor = cx.editor.lock();
                editor.open_sql_tab(label, sql.clone(), Some(conn_id), database);
                activate_tab(&bridge, cx, &mut editor, sql.len());
            }
            // Open-table previews run immediately (TablePro behavior): the
            // same execute entry Ctrl+Enter takes. The preview tab's caret
            // is at 0 → resolve_sql picks the whole statement.
            run_command(window, cx, Command::ExecuteQuery);
        }
        AppEvent::Command(cmd) => run_command(window, cx, cmd),
        AppEvent::QueryStarted { tab } => {
            stash_result(cx, tab, TabResult::Running);
            bridge.set_query_running(true);
            if tab == active_tab(cx) {
                cx.grid.lock().clear(&bridge, "Running…");
            }
        }
        AppEvent::QueryResult {
            tab,
            result,
            elapsed_ms,
        } => {
            let (status, info) = match result.rows_affected {
                Some(n) if result.columns.is_empty() => (
                    format!("{n} rows affected in {elapsed_ms}ms"),
                    format!("{n} rows affected"),
                ),
                _ => {
                    let n = result.rows.len();
                    let suffix = if result.truncated { " (truncated)" } else { "" };
                    (
                        format!("{n} rows in {elapsed_ms}ms{suffix}"),
                        format!("{n} rows{suffix}"),
                    )
                }
            };
            // Stash first — `show_tab_result` reads the slot back. The
            // running flag and history refresh run even when the tab is
            // closed (stash refused): the query DID finish and the Stop
            // button + open panel reflect global state.
            let stashed = stash_result(cx, tab, TabResult::Rows(result, info));
            bridge.set_query_running(cx.backend.any_running());
            refresh_history(window, cx);
            if stashed && tab == active_tab(cx) {
                bridge.set_status(status.into());
                show_tab_result(&bridge, cx, tab);
            }
        }
        AppEvent::QueryError { tab, message } => {
            let stashed = stash_result(cx, tab, TabResult::Failed(message.clone()));
            bridge.set_query_running(cx.backend.any_running());
            refresh_history(window, cx);
            if stashed && tab == active_tab(cx) {
                bridge.set_status(message.into());
                show_tab_result(&bridge, cx, tab);
            }
        }
        AppEvent::QueryCancelled { tab } => {
            let stashed = stash_result(cx, tab, TabResult::Cancelled);
            bridge.set_query_running(cx.backend.any_running());
            if stashed && tab == active_tab(cx) {
                bridge.set_status("Cancelled".into());
                show_tab_result(&bridge, cx, tab);
            }
        }

        AppEvent::HistoryLoaded(items) => {
            bridge.set_history_items(ModelRc::new(VecModel::from(items)));
        }
        AppEvent::CopyHistoryText(text) => {
            GridState::copy_to_clipboard(&mut cx.clipboard.lock(), &bridge, text);
        }

        AppEvent::SavedLoaded(items) => {
            bridge.set_saved_items(ModelRc::new(VecModel::from(items)));
        }
        AppEvent::OpenSavedSql { label, sql } => {
            // Inherit the outgoing tab's connection context — saved queries
            // carry none.
            let (conn_id, database) = {
                let editor = cx.editor.lock();
                (
                    editor.active_tab().conn_id,
                    editor.active_tab().database.clone(),
                )
            };
            {
                let mut editor = cx.editor.lock();
                editor.open_sql_tab(label, sql.clone(), conn_id, database);
                activate_tab(&bridge, cx, &mut editor, sql.len());
            }
        }
    }
}

/// Id of the tab the user is looking at.
fn active_tab(cx: &UiCtx) -> i32 {
    cx.editor.lock().active_tab().id
}

/// Stash `slot` under `tab` — returns false (and drops the event's UI
/// effects) when the tab is already closed, so late results can't leak
/// slots for dead tabs.
fn stash_result(cx: &UiCtx, tab: i32, slot: TabResult) -> bool {
    if !cx.editor.lock().tabs.iter().any(|t| t.id == tab) {
        return false;
    }
    cx.results.lock().insert(tab, slot);
    true
}

/// Load `tab`'s result slot into the visible grid — called after every
/// tab activation and for the active tab's own terminal events. An empty
/// slot means "never run here": cleared grid, no footer text.
fn show_tab_result(bridge: &Bridge, cx: &UiCtx, tab: i32) {
    match cx.results.lock().get(&tab) {
        Some(TabResult::Running) => cx.grid.lock().clear(bridge, "Running…"),
        Some(TabResult::Failed(m)) => cx.grid.lock().clear(bridge, &format!("Error: {m}")),
        Some(TabResult::Cancelled) => cx.grid.lock().clear(bridge, "Query cancelled"),
        // TabResult isn't Clone — rebuild Rows from a reference.
        Some(TabResult::Rows(result, info)) => {
            cx.grid
                .lock()
                .set_result(bridge, result.clone(), info.clone())
        }
        None => cx.grid.lock().clear(bridge, ""),
    }
}

/// Common tail of every tab activation (new/switch/close/open-sql): refresh
/// the strip, load the incoming tab's text + caret, and restore its stashed
/// result into the grid. `cursor` is the byte offset the caret jumps to.
pub(crate) fn activate_tab(
    bridge: &Bridge,
    cx: &Arc<UiCtx>,
    editor: &mut EditorState,
    cursor: usize,
) {
    push_tabs(bridge, editor);
    let jump = editor.cursor_jump(cursor);
    let text = editor.active_tab().text.clone();
    bridge.set_editor_text(text.clone().into());
    bridge.set_line_count(line_count(&text) as i32);
    bridge.set_highlight_spans(spans_model(&text));
    bridge.set_set_cursor(jump);
    show_tab_result(bridge, cx, editor.active_tab().id);
}

/// Auto-refresh (Tasks 6.1/6.2): after a query completes, its history row
/// just landed in storage — re-run the active panel tab's search when the
/// panel is open.
fn refresh_history(window: &MainWindow, cx: &Arc<UiCtx>) {
    let bridge = window.global::<Bridge>();
    if !bridge.get_history_visible() {
        return;
    }
    let filter = bridge.get_history_query().to_string();
    let backend = Arc::clone(&cx.backend);
    let ui = UiHandle::new(window, Arc::clone(cx));
    if bridge.get_history_tab() == 1 {
        cx.handle.spawn(async move {
            backend.saved_list(&filter, ui).await;
        });
    } else {
        let conn_id = cx.editor.lock().active_tab().conn_id;
        cx.handle.spawn(async move {
            backend.history_search(conn_id, &filter, ui).await;
        });
    }
}

/// Ctrl+S / palette "Save query": open the name dialog when there's text
/// worth saving. The dialog's Save calls `Bridge.save-query` with the
/// current editor text — nothing stashed at open time (modal; the editor
/// can't change while it's up).
pub(crate) fn open_save_query_dialog(bridge: &Bridge) {
    if bridge.get_editor_text().trim().is_empty() {
        bridge.set_status("Nothing to save".into());
    } else {
        bridge.set_save_dialog_visible(true);
    }
}

/// Ctrl+Shift+F / toolbar button: show or hide the history panel. Opening
/// runs the first search with the current filter; closing hands focus
/// back to the editor via the .slint watcher.
pub(crate) fn toggle_history(window: &MainWindow, cx: &Arc<UiCtx>) {
    let bridge = window.global::<Bridge>();
    let open = !bridge.get_history_visible();
    bridge.set_history_visible(open);
    if open {
        refresh_history(window, cx);
    }
}

/// Execute one [`Command`] against current editor state.
pub(crate) fn run_command(window: &MainWindow, cx: &Arc<UiCtx>, cmd: Command) {
    let bridge = window.global::<Bridge>();
    match cmd {
        Command::ExecuteQuery => {
            let (sql, conn, db, tab_id) = {
                let mut editor = cx.editor.lock();
                let text = bridge.get_editor_text().to_string();
                let caret = editor.cursor;
                editor.stash(text.clone(), caret);
                let range = resolve_sql(&text, editor.anchor, editor.cursor);
                let sql = range.map(|(s, e)| text[s..e].to_string());
                let tab = &editor.tabs[editor.active];
                (
                    sql,
                    tab.conn_id.or_else(|| cx.backend.last_conn_id()),
                    tab.database.clone(),
                    tab.id,
                )
            };
            let Some(sql) = sql.filter(|s| !s.trim().is_empty()) else {
                bridge.set_status("Nothing to execute".into());
                return;
            };
            let Some(conn_id) = conn else {
                bridge.set_status("No connection — expand a connection first".into());
                return;
            };
            if let Some(t) = cx.editor.lock().tabs.iter_mut().find(|t| t.id == tab_id) {
                t.last_query = Some(sql.clone());
            }
            let backend = Arc::clone(&cx.backend);
            let ui = UiHandle::new(window, Arc::clone(cx));
            let limit = cx.query_limit as usize;
            cx.handle.spawn(async move {
                backend
                    .execute_sql(tab_id, conn_id, db, sql, limit, ui)
                    .await;
            });
            bridge.set_status("Executing…".into());
        }
        Command::NewQuery => {
            let mut editor = cx.editor.lock();
            let caret = editor.cursor;
            editor.stash(bridge.get_editor_text().to_string(), caret);
            editor.new_tab();
            activate_tab(&bridge, cx, &mut editor, 0);
        }
        Command::CloseTab => {
            let mut editor = cx.editor.lock();
            let id = editor.active_tab().id;
            if let Some((_text, cursor)) = editor.close(id, bridge.get_editor_text().to_string()) {
                cx.results.lock().remove(&id);
                activate_tab(&bridge, cx, &mut editor, cursor);
            }
        }
        Command::NextTab => {
            let mut editor = cx.editor.lock();
            if let Some((_id, _text, cursor)) =
                editor.next_tab(bridge.get_editor_text().to_string())
            {
                activate_tab(&bridge, cx, &mut editor, cursor);
            }
        }
        Command::Search => {
            if bridge.get_schema_filter_visible() {
                // Hiding clears the filter so the sidebar comes back whole.
                bridge.set_schema_filter_visible(false);
                let mut tree = cx.tree.lock();
                tree.filter("");
                push_tree(&bridge, &tree);
            } else {
                bridge.set_schema_filter_visible(true);
            }
        }
        Command::SaveQuery => open_save_query_dialog(&bridge),
        Command::OpenPalette => open_palette(&bridge, &cx.tree.lock()),
        Command::Find => bridge.set_status("Find: not yet implemented".into()),
        Command::SearchHistory => toggle_history(window, cx),
        Command::RefreshSchema => {
            let backend = Arc::clone(&cx.backend);
            let ui = UiHandle::new(window, Arc::clone(cx));
            cx.handle.spawn(async move {
                backend.reload_connections(&ui).await;
            });
        }
        // Mapped variants with no key binding in spec §20. OpenConnection
        // never reaches here — the palette emits `open-connection:<id>`
        // tags, not command names; ToggleSidebar has no sidebar state yet.
        Command::OpenConnection | Command::ToggleSidebar => {}
    }
}

/// Rebuild `Bridge.tabs` + `active-tab` from `EditorState`.
pub(crate) fn push_tabs(bridge: &Bridge, editor: &EditorState) {
    let items: Vec<TabItem> = editor
        .tabs
        .iter()
        .map(|t| TabItem {
            id: t.id,
            title: t.title.clone().into(),
        })
        .collect();
    bridge.set_tabs(ModelRc::new(VecModel::from(items)));
    bridge.set_active_tab(editor.active_tab().id);
}

/// `highlight` tokens → Slint model rows.
pub(crate) fn spans_model(text: &str) -> ModelRc<HighlightSpan> {
    ModelRc::new(VecModel::from(highlight_spans(text)))
}

/// Rebuild the whole `Bridge.tree` model from `tree`, applying the stored
/// sidebar filter ([`SchemaTree::view`]). While filtered, rows show the
/// expanded chevron — they're a flat match list, not collapse state.
///
/// ponytail: full `VecModel` snapshot per mutation — schema trees stay small
/// (hundreds of rows); swap to `ModelNotify` row diffs if profiling bites.
pub(crate) fn push_tree(bridge: &Bridge, tree: &SchemaTree) {
    let filtering = tree.filtering();
    let rows: Vec<SlintTreeNode> = tree
        .view()
        .into_iter()
        .map(|n| SlintTreeNode {
            id: n.id,
            depth: i32::from(n.depth),
            kind: n.kind.as_str().into(),
            label: n.label.as_str().into(),
            has_children: n.has_children,
            expanded: n.expanded || filtering,
        })
        .collect();
    bridge.set_tree(ModelRc::new(VecModel::from(rows)));
}

/// Rebuild `Bridge.palette-items`: the static command table + `Connect…` +
/// one `Open connection: <name>` row per visible connection root, fuzzy-
/// filtered by `query` and sorted by match score.
pub(crate) fn palette_model(tree: &SchemaTree, query: &str) -> ModelRc<CommandItem> {
    let mut entries: Vec<(String, String)> = PALETTE_COMMANDS
        .iter()
        .map(|(tag, label, _)| ((*tag).to_owned(), (*label).to_owned()))
        .collect();
    entries.push(("connect".into(), "Connect…".into()));
    entries.extend(
        tree.visible()
            .iter()
            .filter(|n| n.kind == NodeKind::Connection)
            .map(|n| {
                (
                    format!("open-connection:{}", n.id),
                    format!("Open connection: {}", n.label),
                )
            }),
    );
    let mut scored: Vec<(i32, usize, String, String)> = entries
        .into_iter()
        .enumerate()
        .filter_map(|(i, (tag, label))| fuzzy(query, &label).map(|s| (s, i, tag, label)))
        .collect();
    // Stable sort: score ties keep declaration order.
    scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    let items: Vec<CommandItem> = scored
        .into_iter()
        .map(|(_, _, command, label)| CommandItem {
            label: label.into(),
            command: command.into(),
        })
        .collect();
    ModelRc::new(VecModel::from(items))
}

/// Show the palette with a fresh query and the full item list.
pub(crate) fn open_palette(bridge: &Bridge, tree: &SchemaTree) {
    bridge.set_palette_query("".into());
    bridge.set_palette_items(palette_model(tree, ""));
    bridge.set_palette_index(0);
    bridge.set_palette_visible(true);
}

/// Resolve `open-table(id)` to a [`AppEvent::PreviewSql`]: only Table/View
/// rows with a complete nav payload produce one. Pure — the UI thread calls
/// it inside `on_open_table`.
pub(crate) fn open_table_event(tree: &SchemaTree, id: i32, limit: u32) -> Option<AppEvent> {
    let node = tree.visible().get(tree.find(id)?)?;
    if !matches!(node.kind, NodeKind::Table | NodeKind::View) {
        return None;
    }
    let (conn_id, database, schema, table) = (
        node.connection_id?,
        node.database.clone()?,
        node.schema.clone()?,
        node.table.clone()?,
    );
    Some(AppEvent::PreviewSql {
        conn_id,
        database: Some(database),
        sql: preview_sql(&schema, &table, limit),
        label: format!("{schema}.{table}"),
    })
}

/// Table/view/column labels currently in the schema tree — the completion
/// catalog. Column labels carry their `": type"` suffix from the tree row.
pub(crate) fn catalog_labels(tree: &SchemaTree) -> Vec<String> {
    tree.visible()
        .iter()
        .filter(|n| matches!(n.kind, NodeKind::Table | NodeKind::View | NodeKind::Column))
        .map(|n| n.label.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `open-table` fires only for Table/View rows carrying the full nav
    /// payload, and the generated SQL is quoted + limited.
    #[test]
    fn open_table_event_resolves_table_rows() {
        let mut tree = SchemaTree::default();
        tree.set_connections(vec![ConnectionProfile {
            id: ConnectionId(7),
            name: "local".into(),
            host: "127.0.0.1".into(),
            port: 1433,
            database: None,
            username: "sa".into(),
            authentication: datara_domain::AuthenticationMode::SqlPassword,
            encryption: datara_domain::EncryptionMode::Disabled,
            trust_server_certificate: true,
            secret_reference: datara_domain::SecretReference("r".into()),
        }]);
        let root_id = tree.visible()[0].id;
        tree.expand_placeholder(0);
        tree.replace_children(
            root_id,
            vec![TreeNode {
                connection_id: Some(ConnectionId(7)),
                database: Some("master".into()),
                schema: Some("dbo".into()),
                table: Some("users".into()),
                ..TreeNode::new(NodeKind::Table, "dbo.users")
            }],
        );

        // Connection row: not a table.
        assert!(open_table_event(&tree, root_id, 1000).is_none());
        // Unknown id: no event.
        assert!(open_table_event(&tree, 999, 1000).is_none());

        let table_id = tree.visible()[1].id;
        let Some(AppEvent::PreviewSql {
            conn_id,
            database,
            sql,
            label,
        }) = open_table_event(&tree, table_id, 250)
        else {
            panic!("expected PreviewSql");
        };
        assert_eq!(conn_id, ConnectionId(7));
        assert_eq!(
            (database, label),
            (Some("master".into()), "dbo.users".into())
        );
        assert_eq!(sql, "SELECT TOP 250 * FROM [dbo].[users]");
    }
}
