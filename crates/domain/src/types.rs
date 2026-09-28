use serde::{Deserialize, Serialize};

use crate::{ConnectionId, Value};

/// A database on the connected server.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DatabaseInfo {
    pub name: String,
}

/// A schema within a database.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SchemaInfo {
    pub name: String,
}

/// Whether a relation is a base table or a view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TableKind {
    Base,
    View,
}

/// A table or view in a schema.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TableInfo {
    pub schema: String,
    pub name: String,
    pub kind: TableKind,
}

/// A column of a table or view.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ColumnInfo {
    pub name: String,
    pub data_type: String,
    pub nullable: bool,
    pub is_primary_key: bool,
    pub ordinal: u32,
}

/// Full description of a table: columns plus indexes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TableDescription {
    pub schema: String,
    pub name: String,
    pub columns: Vec<ColumnInfo>,
    pub indexes: Vec<IndexInfo>,
}

/// An index on a table.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexInfo {
    pub name: String,
    pub columns: Vec<String>,
    pub is_unique: bool,
}

/// The result of executing a query.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QueryResult {
    pub columns: Vec<QueryColumn>,
    pub rows: Vec<QueryRow>,
    pub rows_affected: Option<u64>,
    pub truncated: bool,
}

/// A column header in a query result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueryColumn {
    pub name: String,
    pub data_type: String,
}

/// One row of a query result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QueryRow {
    pub cells: Vec<Value>,
}

/// A recorded query execution for the history view.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QueryHistoryEntry {
    pub id: i64,
    pub connection_id: ConnectionId,
    pub database: Option<String>,
    pub query: String,
    pub started_at: jiff::Timestamp,
    pub duration_ms: u64,
    pub row_count: u64,
    pub success: bool,
    pub error_message: Option<String>,
}

/// A query saved by the user for reuse.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SavedQuery {
    pub id: i64,
    pub name: String,
    pub query: String,
    pub created_at: jiff::Timestamp,
}

/// UI commands dispatched by shortcuts, menus, and the command palette.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Command {
    ExecuteQuery,
    NewQuery,
    SaveQuery,
    Search,
    OpenPalette,
    Find,
    SearchHistory,
    NextTab,
    CloseTab,
    OpenConnection,
    RefreshSchema,
    ToggleSidebar,
}
