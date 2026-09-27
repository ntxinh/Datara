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

use crate::schema_tree::{SchemaTree, TreeNode};
use crate::{Bridge, MainWindow, TreeNode as SlintTreeNode};
use datara_domain::ConnectionProfile;
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
