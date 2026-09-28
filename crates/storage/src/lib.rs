//! Local persistence layer: connection profiles (metadata only — passwords
//! live in the system secret store, referenced by `SecretReference`), query
//! history, and saved queries on SQLite via sqlx.

mod connections;
mod db;
mod history;
mod saved;

pub use connections::{ConnectionRepo, NewConnection};
pub use db::Storage;
pub use history::HistoryRepo;
pub use saved::SavedRepo;

pub(crate) fn storage_err(e: impl std::fmt::Display) -> datara_domain::DomainError {
    datara_domain::DomainError::Storage {
        message: e.to_string(),
    }
}
