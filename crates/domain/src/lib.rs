//! Core domain types shared by every Datara crate.

mod connection;
mod error;
mod types;
mod value;

pub use connection::{
    AuthenticationMode, ConnectionId, ConnectionProfile, Credentials, EncryptionMode,
    SecretReference,
};
pub use error::{DomainError, Result};
pub use types::{
    ColumnInfo, Command, DatabaseInfo, IndexInfo, QueryColumn, QueryHistoryEntry, QueryResult,
    QueryRow, SavedQuery, SchemaInfo, TableDescription, TableInfo, TableKind,
};
pub use value::Value;
