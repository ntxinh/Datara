//! Shared testcontainers harness for MSSQL integration tests.
//!
//! Other crates needing a live SQL Server include this via
//! `#[path = ".../driver-mssql/tests/common/mod.rs"]` (workspace
//! `cargo test` only runs member-package tests, so the harness lives in a
//! member crate's `tests/` — see top-level `tests/README.md`).
//!
//! Rootless Podman notes: we point testcontainers at the user's podman
//! socket via `DOCKER_HOST` + `TESTCONTAINERS_DOCKER_SOCKET_OVERRIDE`, and
//! disable Ryuk (the cleanup reaper), which is flaky under rootless podman
//! — dropped `ContainerAsync` handles still remove their containers.

use datara_domain::{
    AuthenticationMode, ConnectionId, ConnectionProfile, Credentials, EncryptionMode,
    SecretReference,
};
use testcontainers::{
    core::{IntoContainerPort, WaitFor},
    runners::AsyncRunner,
    ContainerAsync, GenericImage, ImageExt,
};

pub const SA_PASSWORD: &str = "Str0ng!Passw0rd";

/// Point testcontainers at the rootless podman socket. Returns `false` when
/// no socket is usable — callers must then skip, not fail.
pub fn ensure_podman() -> bool {
    if std::env::var("DOCKER_HOST").is_err() {
        // podman rootless socket lives at $XDG_RUNTIME_DIR/podman/podman.sock
        let Ok(dir) = std::env::var("XDG_RUNTIME_DIR") else {
            return false;
        };
        let sock = format!("{dir}/podman/podman.sock");
        if !std::path::Path::new(&sock).exists() {
            return false;
        }
        std::env::set_var("DOCKER_HOST", format!("unix://{sock}"));
        std::env::set_var("TESTCONTAINERS_DOCKER_SOCKET_OVERRIDE", &sock);
        // Ryuk is unreliable under rootless podman; containers are still
        // removed when ContainerAsync drops.
        std::env::set_var("TESTCONTAINERS_RYUK_DISABLED", "true");
    }
    true
}

/// Start a `mcr.microsoft.com/mssql/server:2022-latest` container and return
/// it plus a ready-made profile/credentials pair. `None` = no podman; skip.
/// First run pulls ~1.5 GB.
pub async fn mssql() -> Option<(ContainerAsync<GenericImage>, ConnectionProfile, Credentials)> {
    if !ensure_podman() {
        eprintln!("SKIP: no podman socket");
        return None;
    }
    let image = GenericImage::new("mcr.microsoft.com/mssql/server", "2022-latest")
        .with_exposed_port(1433.tcp())
        .with_wait_for(WaitFor::message_on_stdout(
            "SQL Server is now ready for client connections",
        ))
        .with_env_var("ACCEPT_EULA", "Y")
        .with_env_var("MSSQL_SA_PASSWORD", SA_PASSWORD);
    let container = image.start().await.ok()?;
    let port = container.get_host_port_ipv4(1433).await.ok()?;
    let profile = ConnectionProfile {
        id: ConnectionId(0),
        name: "it".into(),
        host: "127.0.0.1".into(),
        port,
        database: Some("master".into()),
        username: "sa".into(),
        authentication: AuthenticationMode::SqlPassword,
        encryption: EncryptionMode::Preferred,
        trust_server_certificate: true,
        secret_reference: SecretReference("test".into()),
    };
    let creds = Credentials {
        username: "sa".into(),
        password: SA_PASSWORD.into(),
    };
    Some((container, profile, creds))
}

/// Connect with retry: SQL Server logs "ready for client connections" before
/// env-var provisioning (`MSSQL_SA_PASSWORD`) fully lands — a login attempted
/// in that window fails with "Login failed for user 'sa'". Retry auth failures
/// for up to 90 s (slow first boot under parallel containers), propagate
/// everything else immediately.
pub async fn connect(
    profile: &ConnectionProfile,
    creds: &Credentials,
) -> datara_domain::Result<Box<dyn datara_database::DatabaseSession>> {
    use datara_database::DatabaseDriver;
    use datara_domain::DomainError;

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(90);
    loop {
        match datara_driver_mssql::MssqlDriver
            .connect(profile, creds)
            .await
        {
            Err(DomainError::Authentication { .. }) if std::time::Instant::now() < deadline => {
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            }
            other => return other,
        }
    }
}
