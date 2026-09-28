use std::fmt;

use serde::{Deserialize, Serialize};

/// Identifier of a stored connection profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ConnectionId(pub i64);

/// Reference to a secret in the system secret store, e.g. "mssql/7/password".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecretReference(pub String);

/// How the connection authenticates to SQL Server.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AuthenticationMode {
    SqlPassword,
}

/// Whether the connection requires TLS encryption.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EncryptionMode {
    Disabled,
    Preferred,
    Required,
}

/// A saved connection profile. Passwords are never stored here — see
/// `secret_reference`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConnectionProfile {
    pub id: ConnectionId,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub database: Option<String>,
    pub username: String,
    pub authentication: AuthenticationMode,
    pub encryption: EncryptionMode,
    pub trust_server_certificate: bool,
    pub secret_reference: SecretReference,
}

/// Credentials resolved from the secret store at connect time.
#[derive(Clone, Deserialize)]
pub struct Credentials {
    pub username: String,
    pub password: secrecy::SecretString,
}

impl fmt::Debug for Credentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Credentials")
            .field("username", &self.username)
            .field("password", &"***")
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_reference_serde_round_trip() {
        let r = SecretReference("mssql/7/password".into());
        let json = serde_json::to_string(&r).unwrap();
        assert_eq!(json, r#""mssql/7/password""#);
        let back: SecretReference = serde_json::from_str(&json).unwrap();
        assert_eq!(back, r);
    }

    #[test]
    fn credentials_debug_redacts_password() {
        let creds = Credentials {
            username: "sa".into(),
            password: "hunter2".into(),
        };
        let dbg = format!("{creds:?}");
        assert!(dbg.contains("***"));
        assert!(!dbg.contains("hunter2"));
    }

    /// Invariant: a serialized `ConnectionProfile` exposes the secret
    /// reference only — there is no `password` key and no secret material.
    #[test]
    fn connection_profile_json_carries_reference_not_secret() {
        let profile = ConnectionProfile {
            id: ConnectionId(7),
            name: "prod".into(),
            host: "db.internal".into(),
            port: 1433,
            database: Some("appdb".into()),
            username: "sa".into(),
            authentication: AuthenticationMode::SqlPassword,
            encryption: EncryptionMode::Preferred,
            trust_server_certificate: false,
            secret_reference: SecretReference("mssql/7/password".into()),
        };
        let v = serde_json::to_value(&profile).unwrap();
        assert_eq!(v["secret_reference"], "mssql/7/password");
        assert!(v.get("password").is_none());
    }
}
