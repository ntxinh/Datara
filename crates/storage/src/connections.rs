use datara_domain::{
    AuthenticationMode, ConnectionId, ConnectionProfile, DomainError, EncryptionMode, Result,
    SecretReference,
};
use sqlx::FromRow;

use crate::{storage_err, Storage};

/// A connection profile to be stored. `id` is assigned by SQLite;
/// `secret_reference` is derived from it (see [`ConnectionRepo::insert`]).
#[derive(Debug, Clone)]
pub struct NewConnection {
    pub name: String,
    pub host: String,
    pub port: u16,
    pub database: Option<String>,
    pub username: String,
    pub authentication: AuthenticationMode,
    pub encryption: EncryptionMode,
    pub trust_server_certificate: bool,
}

/// Repository over the `connections` table.
pub struct ConnectionRepo<'a> {
    storage: &'a Storage,
}

impl<'a> ConnectionRepo<'a> {
    pub(crate) fn new(storage: &'a Storage) -> Self {
        Self { storage }
    }

    /// List all profiles, ordered by name.
    pub async fn list(&self) -> Result<Vec<ConnectionProfile>> {
        let rows = sqlx::query_as::<_, ConnectionRow>(
            "SELECT id, name, host, port, database, username, auth_mode, encryption, \
             trust_server_certificate, secret_reference FROM connections ORDER BY id",
        )
        .fetch_all(self.storage.pool())
        .await
        .map_err(storage_err)?;
        rows.into_iter().map(ConnectionRow::into_profile).collect()
    }

    /// Fetch one profile by id.
    pub async fn get(&self, id: ConnectionId) -> Result<ConnectionProfile> {
        let row = sqlx::query_as::<_, ConnectionRow>(
            "SELECT id, name, host, port, database, username, auth_mode, encryption, \
             trust_server_certificate, secret_reference FROM connections WHERE id = ?",
        )
        .bind(id.0)
        .fetch_one(self.storage.pool())
        .await
        .map_err(|e| match e {
            sqlx::Error::RowNotFound => DomainError::Storage {
                message: format!("connection {} not found", id.0),
            },
            other => storage_err(other),
        })?;
        row.into_profile()
    }

    /// Insert a profile and return its assigned id.
    ///
    /// `secret_reference` is derived from the id (`mssql/{id}/password`), so
    /// this is a two-step write: insert with a placeholder, then UPDATE the row
    /// with the computed reference.
    pub async fn insert(&self, new: NewConnection) -> Result<ConnectionId> {
        let result = sqlx::query(
            "INSERT INTO connections (name, host, port, database, username, auth_mode, \
             encryption, trust_server_certificate, secret_reference) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, '')",
        )
        .bind(&new.name)
        .bind(&new.host)
        .bind(i64::from(new.port))
        .bind(&new.database)
        .bind(&new.username)
        .bind(auth_mode_str(new.authentication))
        .bind(encryption_str(new.encryption))
        .bind(new.trust_server_certificate)
        .execute(self.storage.pool())
        .await
        .map_err(storage_err)?;
        let id = ConnectionId(result.last_insert_rowid());
        sqlx::query("UPDATE connections SET secret_reference = ? WHERE id = ?")
            .bind(secret_reference_for(id))
            .bind(id.0)
            .execute(self.storage.pool())
            .await
            .map_err(storage_err)?;
        Ok(id)
    }

    /// Update mutable fields of an existing profile. `secret_reference` is
    /// derived from the id and intentionally not rewritten.
    pub async fn update(&self, profile: &ConnectionProfile) -> Result<()> {
        sqlx::query(
            "UPDATE connections SET name = ?, host = ?, port = ?, database = ?, \
             username = ?, auth_mode = ?, encryption = ?, trust_server_certificate = ? \
             WHERE id = ?",
        )
        .bind(&profile.name)
        .bind(&profile.host)
        .bind(i64::from(profile.port))
        .bind(&profile.database)
        .bind(&profile.username)
        .bind(auth_mode_str(profile.authentication))
        .bind(encryption_str(profile.encryption))
        .bind(profile.trust_server_certificate)
        .bind(profile.id.0)
        .execute(self.storage.pool())
        .await
        .map_err(storage_err)?;
        Ok(())
    }

    /// Delete a profile by id.
    pub async fn delete(&self, id: ConnectionId) -> Result<()> {
        sqlx::query("DELETE FROM connections WHERE id = ?")
            .bind(id.0)
            .execute(self.storage.pool())
            .await
            .map_err(storage_err)?;
        Ok(())
    }
}

/// Where the profile's password lives in the system secret store.
fn secret_reference_for(id: ConnectionId) -> String {
    format!("mssql/{}/password", id.0)
}

fn auth_mode_str(mode: AuthenticationMode) -> &'static str {
    match mode {
        AuthenticationMode::SqlPassword => "sql",
    }
}

fn parse_auth_mode(s: &str) -> Result<AuthenticationMode> {
    match s {
        "sql" => Ok(AuthenticationMode::SqlPassword),
        other => Err(DomainError::Storage {
            message: format!("unknown auth_mode '{other}'"),
        }),
    }
}

fn encryption_str(mode: EncryptionMode) -> &'static str {
    match mode {
        EncryptionMode::Disabled => "disabled",
        EncryptionMode::Preferred => "preferred",
        EncryptionMode::Required => "required",
    }
}

fn parse_encryption(s: &str) -> Result<EncryptionMode> {
    match s {
        "disabled" => Ok(EncryptionMode::Disabled),
        "preferred" => Ok(EncryptionMode::Preferred),
        "required" => Ok(EncryptionMode::Required),
        other => Err(DomainError::Storage {
            message: format!("unknown encryption '{other}'"),
        }),
    }
}

#[derive(FromRow)]
struct ConnectionRow {
    id: i64,
    name: String,
    host: String,
    port: i64,
    database: Option<String>,
    username: String,
    auth_mode: String,
    encryption: String,
    trust_server_certificate: bool,
    secret_reference: String,
}

impl ConnectionRow {
    fn into_profile(self) -> Result<ConnectionProfile> {
        Ok(ConnectionProfile {
            id: ConnectionId(self.id),
            name: self.name,
            host: self.host,
            port: u16::try_from(self.port).map_err(storage_err)?,
            database: self.database,
            username: self.username,
            authentication: parse_auth_mode(&self.auth_mode)?,
            encryption: parse_encryption(&self.encryption)?,
            trust_server_certificate: self.trust_server_certificate,
            secret_reference: SecretReference(self.secret_reference),
        })
    }
}
