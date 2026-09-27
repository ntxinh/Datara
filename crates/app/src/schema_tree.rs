//! Flat sidebar schema tree: one `Vec<TreeNode>` in DFS order holding only
//! *visible* rows — expanding splices children in, collapsing drains the
//! subtree, so the Slint model is a straight projection of `nodes` (see
//! [`SchemaTree::visible`]). Pure Rust, no Slint types; `bridge.rs` converts
//! rows for the UI.
//!
//! Hierarchy: Connection → Database → Folder(Tables|Views) → Table/View →
//! Column. MSSQL has no schema level in the tree: `Backend::node_children`
//! fans `list_tables` out over `list_schemas` and labels nodes `schema.name`,
//! keeping the schema in the nav payload for `describe_table`.

use datara_domain::{ConnectionId, ConnectionProfile};

/// What a row represents; drives the fetch in `Backend::node_children`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeKind {
    Connection,
    Database,
    /// "Tables" or "Views" grouping under a database.
    Folder(FolderKind),
    Table,
    View,
    Column,
    /// "Loading…" child inserted by [`SchemaTree::expand_placeholder`].
    Loading,
}

impl NodeKind {
    /// Stable tag sent to the Slint delegate (`TreeNode.kind`).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Connection => "connection",
            Self::Database => "database",
            Self::Folder(FolderKind::Tables) => "folder-tables",
            Self::Folder(FolderKind::Views) => "folder-views",
            Self::Table => "table",
            Self::View => "view",
            Self::Column => "column",
            Self::Loading => "loading",
        }
    }
}

/// Which object list a folder shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FolderKind {
    Tables,
    Views,
}

/// One visible tree row. `id`/`depth` are managed by [`SchemaTree`] —
/// constructor values are placeholders overwritten on insert.
#[derive(Debug, Clone)]
pub struct TreeNode {
    pub id: i32,
    pub depth: u16,
    pub kind: NodeKind,
    pub label: String,
    pub has_children: bool,
    pub expanded: bool,
    // Nav payload carried down from ancestors (used by 3.2+ for open-table).
    pub connection_id: Option<ConnectionId>,
    pub database: Option<String>,
    pub schema: Option<String>,
    pub table: Option<String>,
}

impl TreeNode {
    /// `has_children` defaults from the kind — leaf kinds never expand.
    pub fn new(kind: NodeKind, label: impl Into<String>) -> Self {
        let has_children = !matches!(kind, NodeKind::Column | NodeKind::Loading);
        Self {
            id: 0,
            depth: 0,
            kind,
            label: label.into(),
            has_children,
            expanded: false,
            connection_id: None,
            database: None,
            schema: None,
            table: None,
        }
    }
}

/// The whole sidebar tree, flat. Invariant: a collapsed node has no children
/// in `nodes`, so `expanded` ⇔ "has a subtree here" and `nodes` is exactly
/// the set of visible rows.
#[derive(Debug, Default)]
pub struct SchemaTree {
    nodes: Vec<TreeNode>,
    next_id: i32,
}

impl SchemaTree {
    /// Replace/refresh the root rows. Live roots keep their id, expanded
    /// state and subtree — renamed profiles get a new label; removed
    /// profiles take their subtree with them.
    pub fn set_connections(&mut self, conns: Vec<ConnectionProfile>) {
        let mut rest = std::mem::take(&mut self.nodes);
        let mut nodes = Vec::with_capacity(rest.len() + conns.len());
        for conn in conns {
            let fresh = TreeNode {
                connection_id: Some(conn.id),
                ..TreeNode::new(NodeKind::Connection, conn.name)
            };
            let Some(pos) = rest.iter().position(|n| {
                n.kind == NodeKind::Connection && n.connection_id == fresh.connection_id
            }) else {
                nodes.push(TreeNode {
                    id: self.fresh_id(),
                    ..fresh
                });
                continue;
            };
            let mut node = rest.remove(pos);
            node.label = fresh.label;
            nodes.push(node);
            // Roots are depth-0 Connection rows; a live root's subtree runs
            // up to the next Connection row in `rest`.
            let end = rest
                .iter()
                .position(|n| n.kind == NodeKind::Connection)
                .unwrap_or(rest.len());
            nodes.extend(rest.drain(..end));
        }
        self.nodes = nodes;
    }

    /// Index of the node with `id`, if currently visible.
    pub fn find(&self, id: i32) -> Option<usize> {
        self.nodes.iter().position(|n| n.id == id)
    }

    /// The node at `idx` if it needs a fetch on expand: has children, not
    /// yet expanded (per invariant, ⇒ no children present).
    pub fn expandable(&self, idx: usize) -> Option<&TreeNode> {
        let node = self.nodes.get(idx)?;
        (node.has_children && !node.expanded).then_some(node)
    }

    /// Mark `idx` expanded and splice in a "Loading…" child; the real
    /// children arrive via [`SchemaTree::replace_children`]. No-op on nodes
    /// that can't expand.
    pub fn expand_placeholder(&mut self, idx: usize) {
        if self.expandable(idx).is_none() {
            return;
        }
        self.nodes[idx].expanded = true;
        let mut loading = TreeNode::new(NodeKind::Loading, "Loading…");
        loading.id = self.fresh_id();
        loading.depth = self.nodes[idx].depth + 1;
        self.nodes.insert(idx + 1, loading);
    }

    /// Splice `children` under the expanded parent `parent_id`, replacing
    /// any current subtree (the Loading placeholder). Ids/depths of the
    /// passed nodes are overwritten. Results arriving after the parent was
    /// collapsed are discarded — children of a collapsed node are invisible
    /// and would resurrect as duplicates on re-expand.
    pub fn replace_children(&mut self, parent_id: i32, mut children: Vec<TreeNode>) {
        let Some(pos) = self.find(parent_id) else {
            return;
        };
        if !self.nodes[pos].expanded {
            return;
        }
        let depth = self.nodes[pos].depth + 1;
        let start = pos + 1;
        let end = self.subtree_end(pos);
        for child in &mut children {
            child.id = self.fresh_id();
            child.depth = depth;
        }
        self.nodes.splice(start..end, children);
    }

    /// Remove the node's subtree; the node itself stays as a collapsed row.
    pub fn collapse(&mut self, idx: usize) {
        if let Some(node) = self.nodes.get_mut(idx) {
            node.expanded = false;
        } else {
            return;
        }
        let end = self.subtree_end(idx);
        self.nodes.drain(idx + 1..end);
    }

    /// All visible rows — the flat projection the UI renders.
    pub fn visible(&self) -> &[TreeNode] {
        &self.nodes
    }

    fn fresh_id(&mut self) -> i32 {
        self.next_id += 1;
        self.next_id
    }

    /// One past the last descendant of `nodes[pos]` (or `pos + 1` when it
    /// has none).
    fn subtree_end(&self, pos: usize) -> usize {
        let depth = self.nodes[pos].depth;
        self.nodes[pos + 1..]
            .iter()
            .position(|n| n.depth <= depth)
            .map(|rel| pos + 1 + rel)
            .unwrap_or(self.nodes.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use datara_domain::{AuthenticationMode, EncryptionMode, SecretReference};

    fn profile(id: i64, name: &str) -> ConnectionProfile {
        ConnectionProfile {
            id: ConnectionId(id),
            name: name.to_owned(),
            host: "h".into(),
            port: 1433,
            database: None,
            username: "u".into(),
            authentication: AuthenticationMode::SqlPassword,
            encryption: EncryptionMode::Preferred,
            trust_server_certificate: true,
            secret_reference: SecretReference(format!("mssql/{id}/password")),
        }
    }

    fn child(kind: NodeKind, label: &str) -> TreeNode {
        TreeNode::new(kind, label)
    }

    fn tree() -> SchemaTree {
        let mut t = SchemaTree::default();
        t.set_connections(vec![profile(1, "prod"), profile(2, "dev")]);
        t
    }

    #[test]
    fn expand_inserts_loading_placeholder() {
        let mut t = tree();
        let conn_id = t.visible()[0].id;
        t.expand_placeholder(0);
        assert!(t.visible()[0].expanded);
        assert_eq!(t.visible().len(), 3);
        assert_eq!(t.visible()[1].kind, NodeKind::Loading);
        assert_eq!(t.visible()[1].depth, 1);
        assert_eq!(t.visible()[1].label, "Loading…");
        assert!(t.find(conn_id).is_some());
    }

    #[test]
    fn replace_children_swaps_placeholder_for_rows() {
        let mut t = tree();
        t.expand_placeholder(0);
        let conn_id = t.visible()[0].id;
        t.replace_children(
            conn_id,
            vec![
                child(NodeKind::Database, "master"),
                child(NodeKind::Database, "app"),
            ],
        );
        let labels: Vec<_> = t.visible().iter().map(|n| n.label.as_str()).collect();
        assert_eq!(labels, ["prod", "master", "app", "dev"]);
        assert_eq!(t.visible()[1].depth, 1);
        assert_eq!(t.visible()[2].depth, 1);
        // Unique ids across the whole tree.
        let ids: Vec<_> = t.visible().iter().map(|n| n.id).collect();
        let mut dedup = ids.clone();
        dedup.dedup();
        assert_eq!(
            ids.len(),
            dedup.iter().collect::<std::collections::HashSet<_>>().len()
        );
    }

    #[test]
    fn collapse_removes_subtree_only() {
        let mut t = tree();
        t.expand_placeholder(0);
        let conn_id = t.visible()[0].id;
        t.replace_children(conn_id, vec![child(NodeKind::Database, "master")]);
        // Expand the database too — its subtree must die with the parent's.
        t.expand_placeholder(1);
        let db_id = t.visible()[1].id;
        t.replace_children(
            db_id,
            vec![child(NodeKind::Folder(FolderKind::Tables), "Tables")],
        );
        assert_eq!(t.visible().len(), 4);

        t.collapse(0);
        assert!(!t.visible()[0].expanded);
        let labels: Vec<_> = t.visible().iter().map(|n| n.label.as_str()).collect();
        assert_eq!(labels, ["prod", "dev"]);
        assert!(t.find(db_id).is_none());
    }

    #[test]
    fn depths_track_nesting() {
        let mut t = tree();
        t.expand_placeholder(0);
        let conn = t.visible()[0].id;
        t.replace_children(conn, vec![child(NodeKind::Database, "db")]);
        t.expand_placeholder(1);
        let db = t.visible()[1].id;
        t.replace_children(
            db,
            vec![child(NodeKind::Folder(FolderKind::Tables), "Tables")],
        );
        t.expand_placeholder(2);
        let folder = t.visible()[2].id;
        t.replace_children(folder, vec![child(NodeKind::Table, "dbo.t")]);
        t.expand_placeholder(3);
        let table = t.visible()[3].id;
        t.replace_children(table, vec![child(NodeKind::Column, "id")]);
        let depths: Vec<_> = t.visible().iter().map(|n| n.depth).collect();
        assert_eq!(depths, [0, 1, 2, 3, 4, 0]);
    }

    #[test]
    fn leaves_and_expanded_nodes_are_not_expandable() {
        let mut t = tree();
        let mut leaf = child(NodeKind::Column, "id");
        leaf.has_children = false;
        t.expand_placeholder(0);
        t.replace_children(t.visible()[0].id, vec![leaf]);
        assert!(t.expandable(1).is_none()); // leaf
        assert!(t.expandable(0).is_none()); // already expanded
    }

    #[test]
    fn replace_after_collapse_is_discarded() {
        let mut t = tree();
        t.expand_placeholder(0);
        let conn_id = t.visible()[0].id;
        t.collapse(0);
        t.replace_children(conn_id, vec![child(NodeKind::Database, "late")]);
        assert_eq!(t.visible().len(), 2);
        // Re-expand: fresh placeholder, not stale children.
        t.expand_placeholder(0);
        assert_eq!(t.visible()[1].kind, NodeKind::Loading);
    }

    #[test]
    fn set_connections_preserves_live_subtrees() {
        let mut t = tree();
        t.expand_placeholder(0);
        let conn_id = t.visible()[0].id;
        t.replace_children(conn_id, vec![child(NodeKind::Database, "master")]);
        // Reload: prod renamed, dev deleted, staging added.
        t.set_connections(vec![profile(1, "prod2"), profile(3, "staging")]);
        let labels: Vec<_> = t.visible().iter().map(|n| n.label.as_str()).collect();
        assert_eq!(labels, ["prod2", "master", "staging"]);
        assert!(t.visible()[0].expanded);
        assert_eq!(t.visible()[1].depth, 1);
        assert_eq!(t.visible()[2].depth, 0);
    }
}
