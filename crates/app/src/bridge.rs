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

use crate::schema_tree::{NodeKind, SchemaTree, TreeNode};
use crate::services::preview_sql;
use crate::{Bridge, MainWindow, TreeNode as SlintTreeNode};
use datara_domain::{ConnectionId, ConnectionProfile};
use slint::{ComponentHandle, ModelRc, VecModel, Weak};

/// Events flowing core → UI.
pub enum AppEvent {
    /// Full connection list refresh (initial load, after save/delete).
    ConnectionsLoaded(Vec<ConnectionProfile>),
    /// Free-form status line shown at the bottom of the sidebar.
    Status(String),
    /// Dialog Test button finished: `Ok(())` or the error's `Display`.
    ConnectTestResult(Result<(), String>),
    /// `toggle-node` fetch finished; splices under the expanded parent.
    TreeChildren {
        parent_id: i32,
        result: Result<Vec<TreeNode>, String>,
    },
    /// `open-table` resolved a row to its preview SELECT. Carries the full
    /// nav payload so the Phase 5 grid can consume the same event verbatim.
    PreviewSql {
        conn_id: ConnectionId,
        database: String,
        schema: String,
        table: String,
        sql: String,
        label: String,
    },
}

/// Weak handle to the window, safe to move into Tokio tasks.
#[derive(Clone, Default)]
pub struct UiHandle {
    weak: Weak<MainWindow>,
    tree: Arc<Mutex<SchemaTree>>,
}

impl UiHandle {
    pub fn new(window: &MainWindow, tree: Arc<Mutex<SchemaTree>>) -> Self {
        Self {
            weak: window.as_weak(),
            tree,
        }
    }

    /// Post `event` onto the Slint event loop; no-op if the window is gone.
    pub fn dispatch(&self, event: AppEvent) {
        let weak = self.weak.clone();
        let tree = Arc::clone(&self.tree);
        let _ = slint::invoke_from_event_loop(move || {
            let Some(window) = weak.upgrade() else {
                return;
            };
            apply(&window, &tree, event);
        });
    }
}

fn apply(window: &MainWindow, tree: &Mutex<SchemaTree>, event: AppEvent) {
    let bridge = window.global::<Bridge>();
    match event {
        AppEvent::ConnectionsLoaded(profiles) => {
            let mut tree = tree.lock();
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
            let mut tree = tree.lock();
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
            schema,
            table,
            sql,
            label,
        } => {
            // Nav payload is logged (not rendered) — Phase 5's grid consumes
            // this event directly and re-derives the target from it.
            tracing::debug!(?conn_id, database, schema, table, "open-table");
            bridge.set_preview_sql(sql.into());
            bridge.set_status(format!("Preview: {label}").into());
        }
    }
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
        schema,
        table,
    })
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
            schema,
            table,
            sql,
            ..
        }) = open_table_event(&tree, table_id, 250)
        else {
            panic!("expected PreviewSql");
        };
        assert_eq!(conn_id, ConnectionId(7));
        assert_eq!(
            (database, schema, table),
            ("master".into(), "dbo".into(), "users".into())
        );
        assert_eq!(sql, "SELECT TOP 250 * FROM [dbo].[users]");
    }
}
