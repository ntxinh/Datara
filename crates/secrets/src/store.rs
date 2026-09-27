//! Secret Service store implementation.

use std::collections::HashMap;

use datara_domain::{DomainError, Result, SecretReference};
use secrecy::{ExposeSecret, SecretString};
use secret_service::{EncryptionType, SecretService};

const APPLICATION_ATTRIBUTE: &str = "application";
const APPLICATION_ID: &str = "io.github.ntxinh.Datara";
const REFERENCE_ATTRIBUTE: &str = "secret-reference";

/// Store backed by the session Secret Service (GNOME Keyring & co.).
///
/// Requires a running Secret Service provider on the session D-Bus. Secrets
/// are stored in the default collection, which is unlocked on [`connect`]
/// if needed.
pub struct SecretStore {
    ss: SecretService<'static>,
}

impl SecretStore {
    /// Connect to the session Secret Service and unlock the default
    /// collection if it is locked (may prompt the user once).
    pub async fn connect() -> Result<Self> {
        let ss = SecretService::connect(EncryptionType::Dh)
            .await
            .map_err(|e| secret_err("connect to secret service", e))?;
        let collection = ss
            .get_default_collection()
            .await
            .map_err(|e| secret_err("get default collection", e))?;
        if collection
            .is_locked()
            .await
            .map_err(|e| secret_err("check default collection lock", e))?
        {
            collection
                .unlock()
                .await
                .map_err(|e| secret_err("unlock default collection", e))?;
        }
        Ok(Self { ss })
    }

    /// Store `secret` under `reference` with a human-readable `label`,
    /// replacing any existing item for the same reference.
    pub async fn save(
        &self,
        reference: &SecretReference,
        label: &str,
        secret: &SecretString,
    ) -> Result<()> {
        let collection = self
            .ss
            .get_default_collection()
            .await
            .map_err(|e| secret_err("get default collection", e))?;
        collection
            .create_item(
                label,
                attributes(reference),
                secret.expose_secret().as_bytes(),
                true,
                "text/plain",
            )
            .await
            .map_err(|e| secret_err("store secret", e))?;
        Ok(())
    }

    /// Load the secret stored for `reference`. Returns a "not found"
    /// [`DomainError::Secret`] when no unlocked item matches.
    pub async fn load(&self, reference: &SecretReference) -> Result<SecretString> {
        let items = self
            .ss
            .search_items(attributes(reference))
            .await
            .map_err(|e| secret_err("search secret", e))?;
        let item = match items.unlocked.first() {
            Some(item) => item,
            None if items.locked.is_empty() => {
                return Err(DomainError::Secret {
                    message: format!("no secret stored for {}", reference.0),
                });
            }
            None => {
                return Err(DomainError::Secret {
                    message: format!("secret for {} is locked", reference.0),
                });
            }
        };
        let bytes = item
            .get_secret()
            .await
            .map_err(|e| secret_err("read secret", e))?;
        let text =
            String::from_utf8(bytes).map_err(|e| secret_err("decode stored secret as UTF-8", e))?;
        Ok(SecretString::from(text))
    }

    /// Delete every item stored for `reference`. Absent items are not an error.
    pub async fn delete(&self, reference: &SecretReference) -> Result<()> {
        let items = self
            .ss
            .search_items(attributes(reference))
            .await
            .map_err(|e| secret_err("search secret", e))?;
        for item in items.unlocked.iter().chain(items.locked.iter()) {
            item.delete()
                .await
                .map_err(|e| secret_err("delete secret", e))?;
        }
        Ok(())
    }
}

/// Item attributes identifying a Datara-owned secret.
fn attributes(reference: &SecretReference) -> HashMap<&'static str, &str> {
    HashMap::from([
        (APPLICATION_ATTRIBUTE, APPLICATION_ID),
        (REFERENCE_ATTRIBUTE, reference.0.as_str()),
    ])
}

/// Map a secret-service error to a user-facing [`DomainError::Secret`].
/// `action` names what was being attempted; the source error's text may name
/// items but never carries secret material, so it is safe to include.
fn secret_err(action: &str, e: impl std::fmt::Display) -> DomainError {
    DomainError::Secret {
        message: format!("failed to {action}: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attributes_tag_datara_and_reference() {
        let reference = SecretReference("mssql/7/password".into());
        let attrs = attributes(&reference);
        assert_eq!(attrs.len(), 2);
        assert_eq!(attrs[APPLICATION_ATTRIBUTE], "io.github.ntxinh.Datara");
        assert_eq!(attrs[REFERENCE_ATTRIBUTE], "mssql/7/password");
    }

    #[test]
    fn secret_err_names_action_and_wraps_source() {
        let err = secret_err("unlock default collection", "dbus gone");
        assert!(matches!(err, DomainError::Secret { .. }));
        let msg = err.to_string();
        assert!(msg.contains("unlock default collection"));
        assert!(msg.contains("dbus gone"));
    }

    /// Round-trips a real secret through the running Secret Service.
    /// Requires a session D-Bus + GNOME Keyring; gated behind
    /// `DATARA_LIVE_SECRETS=1`, skipped otherwise.
    #[tokio::test]
    async fn live_round_trip() {
        if std::env::var("DATARA_LIVE_SECRETS").ok().as_deref() != Some("1") {
            eprintln!("skipping: set DATARA_LIVE_SECRETS=1 to run against a live Secret Service");
            return;
        }
        let store = SecretStore::connect().await.unwrap();
        let reference = SecretReference(format!("test/{}", std::process::id()));
        let secret = SecretString::from("hunter2");

        store
            .save(&reference, "Datara test secret", &secret)
            .await
            .unwrap();
        let loaded = store.load(&reference).await.unwrap();
        assert_eq!(loaded.expose_secret(), "hunter2");
        store.delete(&reference).await.unwrap();

        let err = store.load(&reference).await.unwrap_err();
        assert!(matches!(err, DomainError::Secret { .. }));
        assert!(err.to_string().contains(&reference.0));
    }
}
