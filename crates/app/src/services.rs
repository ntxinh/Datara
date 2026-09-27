//! Process-wide services: Tokio runtime, storage, secret store, and the
//! shared [`DatabaseService`]. Created once on startup, then shared with the
//! Slint UI as `Arc<AppServices>`; backend work is spawned onto `runtime`
//! from UI callbacks (see `ui.rs`), results return via `UiHandle`.

use std::sync::Arc;

use datara_config::{AppConfig, AppPaths};
use datara_database::DatabaseService;
use datara_domain::{
    AuthenticationMode, ConnectionId, ConnectionProfile, Credentials, EncryptionMode,
    Result as DomainResult, SecretReference,
};
use datara_driver_mssql::MssqlDriver;
use datara_secrets::SecretStore;
use datara_storage::{NewConnection, Storage};
use secrecy::SecretString;
use tokio::runtime::Runtime;

use crate::bridge::{AppEvent, UiHandle};
use crate::ConnForm;

/// Multi-thread runtime for all backend I/O. Slint owns the main thread.
pub fn spawn_runtime() -> DomainResult<Runtime> {
    tokio::runtime::Builder::new_multi_thread()
        .thread_name("datara-db")
        .enable_all()
        .build()
        .map_err(|e| datara_domain::DomainError::Driver {
            message: format!("cannot start tokio runtime: {e}"),
        })
}

/// The async backend, minus the runtime: everything `runtime.spawn`ed tasks
/// need. Passed to the Tokio world as `Arc<Backend>` (`Runtime` itself stays
/// in [`AppServices`] on the UI side).
pub struct Backend {
    pub storage: Arc<Storage>,
    pub secrets: Arc<SecretStore>,
    pub service: Arc<DatabaseService<MssqlDriver>>,
}

/// Long-lived application state: runtime + backend + config.
pub struct AppServices {
    pub runtime: Runtime,
    pub backend: Arc<Backend>,
    pub config: AppConfig,
}

impl AppServices {
    /// Resolve XDG paths, load config, open SQLite, connect the Secret
    /// Service. Synchronous wrapper around the async init.
    pub fn init() -> anyhow::Result<Self> {
        let runtime = spawn_runtime()?;
        let paths = AppPaths::new()?;
        let config = AppConfig::load(&paths)?;
        let storage = Arc::new(runtime.block_on(Storage::open(&paths.app_db()))?);
        let secrets = Arc::new(runtime.block_on(SecretStore::connect())?);
        let service = Arc::new(DatabaseService::<MssqlDriver>::new(
            Arc::clone(&storage),
            Arc::clone(&secrets),
        ));
        let backend = Arc::new(Backend {
            storage,
            secrets,
            service,
        });
        Ok(Self {
            runtime,
            backend,
            config,
        })
    }
}

fn encryption_mode(s: &str) -> EncryptionMode {
    match s {
        "disabled" => EncryptionMode::Disabled,
        "required" => EncryptionMode::Required,
        _ => EncryptionMode::Preferred,
    }
}

/// Profile fields that come straight from the dialog (no id/secret ref).
fn form_fields(form: &ConnForm) -> DomainResult<NewConnection> {
    let port = u16::try_from(form.port).map_err(|_| datara_domain::DomainError::Storage {
        message: format!("invalid port {}", form.port),
    })?;
    Ok(NewConnection {
        name: form.name.to_string(),
        host: form.host.to_string(),
        port,
        database: match form.database.as_str() {
            "" => None,
            s => Some(s.to_owned()),
        },
        username: form.username.to_string(),
        authentication: AuthenticationMode::SqlPassword,
        encryption: encryption_mode(&form.encryption),
        trust_server_certificate: form.trust_cert,
    })
}

impl Backend {
    /// Load all profiles and push them to the sidebar.
    pub async fn reload_connections(&self, ui: &UiHandle) {
        match self.storage.connections().list().await {
            Ok(list) => ui.dispatch(AppEvent::ConnectionsLoaded(list)),
            Err(e) => ui.dispatch(AppEvent::Status(format!("Load failed: {e}"))),
        }
    }

    /// Save flow: insert the profile row, store its password under the
    /// derived secret reference, reload the list. On secret-save failure the
    /// row is rolled back — a profile without its secret is worse than none.
    pub async fn save_connection(&self, form: ConnForm, ui: UiHandle) {
        match self.save_inner(form).await {
            Ok(()) => {
                ui.dispatch(AppEvent::Status("Connection saved".into()));
                self.reload_connections(&ui).await;
            }
            Err(e) => ui.dispatch(AppEvent::Status(format!("Save failed: {e}"))),
        }
    }

    async fn save_inner(&self, form: ConnForm) -> DomainResult<()> {
        let new = form_fields(&form)?;
        let password = SecretString::from(form.password.to_string());
        let repo = self.storage.connections();
        let id = repo.insert(new).await?;
        // The reference is derived from the id by the repo; read it back
        // instead of duplicating the format.
        let reference = repo.get(id).await?.secret_reference;
        if let Err(e) = self
            .secrets
            .save(&reference, &format!("Datara — {}", form.name), &password)
            .await
        {
            let _ = repo.delete(id).await;
            return Err(e);
        }
        Ok(())
    }

    /// Test button: attempt a real connection with the form's fields, report
    /// the outcome into the dialog's `test-result` property.
    pub async fn test_form(&self, form: ConnForm, ui: UiHandle) {
        let result = match form_fields(&form).and_then(|new| self.form_profile(&form, new)) {
            Ok((profile, creds)) => self
                .service
                .test_connection(&profile, &creds)
                .await
                .map_err(|e| e.to_string()),
            Err(e) => Err(e.to_string()),
        };
        ui.dispatch(AppEvent::ConnectTestResult(result));
    }

    /// Wrap the dialog fields in a throwaway `ConnectionProfile` (id/secret
    /// ref are placeholders — nothing is persisted for a test).
    fn form_profile(
        &self,
        form: &ConnForm,
        new: NewConnection,
    ) -> DomainResult<(ConnectionProfile, Credentials)> {
        let profile = ConnectionProfile {
            id: ConnectionId(0),
            name: new.name,
            host: new.host,
            port: new.port,
            database: new.database,
            username: new.username.clone(),
            authentication: new.authentication,
            encryption: new.encryption,
            trust_server_certificate: new.trust_server_certificate,
            secret_reference: SecretReference(String::new()),
        };
        let creds = Credentials {
            username: new.username,
            password: SecretString::from(form.password.to_string()),
        };
        Ok((profile, creds))
    }

    /// Sidebar Connect: open (or reuse) a pooled session for `id`.
    pub async fn connect_profile(&self, id: i32, ui: UiHandle) {
        let id = ConnectionId(i64::from(id));
        match self.service.session(id).await {
            Ok(_) => ui.dispatch(AppEvent::Connected(id)),
            Err(e) => ui.dispatch(AppEvent::ConnectFailed {
                id,
                message: e.to_string(),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    //! End-to-end smoke for the save flow: profile row + keyring item must
    //! persist together. Needs a session Secret Service, so it's ignored by
    //! default — run with:
    //!   DATARA_DATA_DIR=$(mktemp -d) cargo test -p datara -- --ignored

    use super::*;
    use secrecy::ExposeSecret;

    #[test]
    #[ignore = "needs a live Secret Service; run manually"]
    fn smoke_save_persists_profile_and_secret() {
        let tmp = std::env::temp_dir().join(format!("datara-smoke-{}", std::process::id()));
        std::env::set_var("DATARA_DATA_DIR", &tmp);

        let svc = AppServices::init().expect("services init");
        let form = ConnForm {
            name: "smoke".into(),
            host: "127.0.0.1".into(),
            port: 11433,
            database: "master".into(),
            username: "sa".into(),
            password: "smoke-password".into(),
            encryption: "disabled".into(),
            trust_cert: true,
        };

        let (id, list) = svc.runtime.block_on(async {
            svc.backend.save_connection(form, UiHandle::default()).await;
            let list = svc.backend.storage.connections().list().await.unwrap();
            let row = list.iter().find(|c| c.name == "smoke").unwrap().clone();
            let secret = svc
                .backend
                .secrets
                .load(&row.secret_reference)
                .await
                .unwrap();
            assert_eq!(secret.expose_secret(), "smoke-password");
            if std::env::var_os("DATARA_SMOKE_KEEP").is_none() {
                // Clean up: delete row + secret so real state stays untouched.
                svc.backend
                    .storage
                    .connections()
                    .delete(row.id)
                    .await
                    .unwrap();
                svc.backend
                    .secrets
                    .delete(&row.secret_reference)
                    .await
                    .unwrap();
            }
            (row.id, list.len())
        });

        assert!(id.0 > 0);
        assert_eq!(list, 1);
        if std::env::var_os("DATARA_SMOKE_KEEP").is_none() {
            std::fs::remove_dir_all(&tmp).ok();
        }
        println!("smoke data dir: {}", tmp.display());
    }
}
