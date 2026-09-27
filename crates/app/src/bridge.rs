//! Slint ↔ Tokio boundary. Backend tasks spawn on the Tokio runtime and
//! report back through [`UiHandle::dispatch`], which hops onto the Slint
//! event loop (`invoke_from_event_loop`) before touching the window.
//!
//! The sidebar's [`SchemaTree`] lives behind `Arc<Mutex<_>>` shared between
//! `UiHandle` and the UI callbacks — mutex rather than `Rc<RefCell>` because
//! `UiHandle` must stay `Send` for `dispatch` (Tokio tasks hold it). Only
//! the UI thread ever takes the lock; Tokio tasks ship results back as
//! [`AppEvent`]s.

use std::sync::Arc;

use parking_lot::Mutex;

use crate::editor_ui::{highlight_spans, line_count, statement_to_run, EditorState};
use crate::schema_tree::{NodeKind, SchemaTree, TreeNode};
use crate::services::{preview_sql, Backend};
use crate::{Bridge, HighlightSpan, MainWindow, TabItem, TreeNode as SlintTreeNode};
use datara_domain::{Command, ConnectionId, ConnectionProfile};
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
    /// `open-table` resolved a row to its preview SELECT — opens a tab.
    PreviewSql {
        conn_id: ConnectionId,
        database: String,
        sql: String,
        label: String,
    },
    /// A keyboard command (mpsc → event loop → here).
    Command(Command),
    /// Query finished: row count or the error text.
    QueryDone(Result<u64, String>),
}

/// Everything `apply` needs on the UI thread. Shared between `UiHandle`
/// (Tokio side) and the UI callbacks; mutexes rather than `RefCell` because
/// `UiHandle` must stay `Send` for `dispatch`.
pub struct UiCtx {
    pub tree: Mutex<SchemaTree>,
    pub editor: Mutex<EditorState>,
    pub backend: Arc<Backend>,
    /// Runtime handle to spawn backend work from the UI thread.
    pub handle: tokio::runtime::Handle,
    /// `[query] default_limit` from config.
    pub query_limit: u32,
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

fn apply(window: &MainWindow, cx: &Arc<UiCtx>, event: AppEvent) {
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
            let mut editor = cx.editor.lock();
            editor.open_sql_tab(
                label.clone(),
                sql.clone(),
                Some(conn_id),
                Some(database.clone()),
            );
            push_tabs(&bridge, &editor);
            let jump = editor.cursor_jump(sql.len());
            bridge.set_editor_text(sql.clone().into());
            bridge.set_line_count(line_count(&sql) as i32);
            bridge.set_highlight_spans(spans_model(&sql));
            bridge.set_set_cursor(jump);
            bridge.set_status(format!("Preview: {label}").into());
        }
        AppEvent::Command(cmd) => run_command(window, cx, cmd),
        AppEvent::QueryDone(Ok(rows)) => {
            bridge.set_status(format!("{rows} rows").into());
            bridge.set_results_text(format!("{rows} rows returned").into());
        }
        AppEvent::QueryDone(Err(e)) => {
            bridge.set_status(format!("Query failed: {e}").into());
            bridge.set_results_text(format!("Error: {e}").into());
        }
    }
}

/// Execute one [`Command`] against current editor state.
fn run_command(window: &MainWindow, cx: &Arc<UiCtx>, cmd: Command) {
    let bridge = window.global::<Bridge>();
    match cmd {
        Command::ExecuteQuery => {
            let (sql, conn, db) = {
                let mut editor = cx.editor.lock();
                let text = bridge.get_editor_text().to_string();
                let caret = editor.cursor;
                editor.stash(text.clone(), caret);
                let range = statement_to_run(&text, editor.anchor, editor.cursor);
                let tab = editor.active_tab();
                (
                    range.map(|(s, e)| text[s..e].to_string()),
                    tab.conn_id.or_else(|| cx.backend.last_conn_id()),
                    tab.database.clone(),
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
            let backend = Arc::clone(&cx.backend);
            let ui = UiHandle::new(window, Arc::clone(cx));
            let limit = cx.query_limit as usize;
            cx.handle.spawn(async move {
                backend.execute_sql(conn_id, db, sql, limit, ui).await;
            });
            bridge.set_status("Executing…".into());
        }
        Command::NewQuery => {
            let mut editor = cx.editor.lock();
            let caret = editor.cursor;
            editor.stash(bridge.get_editor_text().to_string(), caret);
            editor.new_tab();
            push_tabs(&bridge, &editor);
            let jump = editor.cursor_jump(0);
            bridge.set_editor_text("".into());
            bridge.set_line_count(1);
            bridge.set_highlight_spans(ModelRc::new(VecModel::<HighlightSpan>::from(Vec::new())));
            bridge.set_set_cursor(jump);
        }
        Command::CloseTab => {
            let mut editor = cx.editor.lock();
            let id = editor.active_tab().id;
            if let Some((text, cursor)) = editor.close(id, bridge.get_editor_text().to_string()) {
                push_tabs(&bridge, &editor);
                let jump = editor.cursor_jump(cursor);
                bridge.set_editor_text(text.clone().into());
                bridge.set_line_count(line_count(&text) as i32);
                bridge.set_highlight_spans(spans_model(&text));
                bridge.set_set_cursor(jump);
            }
        }
        Command::NextTab => {
            let mut editor = cx.editor.lock();
            if let Some((_id, text, cursor)) = editor.next_tab(bridge.get_editor_text().to_string())
            {
                push_tabs(&bridge, &editor);
                let jump = editor.cursor_jump(cursor);
                bridge.set_editor_text(text.clone().into());
                bridge.set_line_count(line_count(&text) as i32);
                bridge.set_highlight_spans(spans_model(&text));
                bridge.set_set_cursor(jump);
            }
        }
        Command::Search => {
            bridge.set_schema_filter_visible(!bridge.get_schema_filter_visible());
        }
        Command::SaveQuery => bridge.set_status("Save query: not yet implemented".into()),
        Command::OpenPalette => bridge.set_status("Palette: not yet implemented".into()),
        Command::Find => bridge.set_status("Find: not yet implemented".into()),
        Command::SearchHistory => bridge.set_status("History search: not yet implemented".into()),
        // Mapped variants with no key binding in spec §20.
        Command::OpenConnection | Command::RefreshSchema | Command::ToggleSidebar => {}
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

/// Rebuild the whole `Bridge.tree` model from `tree`.
///
/// ponytail: full `VecModel` snapshot per mutation — schema trees stay small
/// (hundreds of rows); swap to `ModelNotify` row diffs if profiling bites.
pub(crate) fn push_tree(bridge: &Bridge, tree: &SchemaTree) {
    let rows: Vec<SlintTreeNode> = tree
        .visible()
        .iter()
        .map(|n| SlintTreeNode {
            id: n.id,
            depth: i32::from(n.depth),
            kind: n.kind.as_str().into(),
            label: n.label.as_str().into(),
            has_children: n.has_children,
            expanded: n.expanded,
        })
        .collect();
    bridge.set_tree(ModelRc::new(VecModel::from(rows)));
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
        database,
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
        assert_eq!((database, label), ("master".into(), "dbo.users".into()));
        assert_eq!(sql, "SELECT TOP 250 * FROM [dbo].[users]");
    }
}
