//! Datara secrets crate.
//!
//! Connection passwords live in the system Secret Service (GNOME Keyring,
//! KeePassXC, …) over D-Bus — never in `datara-storage` or on disk. Items are
//! tagged with `{"application": "io.github.ntxinh.Datara", "secret-reference":
//! <ref>}` so they stay visible/manageable in Seahorse and friends.
//!
//! Requires a session D-Bus and a running Secret Service provider; without one
//! every operation fails with [`datara_domain::DomainError::Secret`].

mod store;

pub use store::SecretStore;
