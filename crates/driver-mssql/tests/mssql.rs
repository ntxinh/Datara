//! Live integration tests against a containerized SQL Server (podman +
//! testcontainers). Every container test skips cleanly when no podman socket
//! is available; `unreachable_host_is_connection_error` needs no container.

mod common;

use common::{connect, mssql, SA_PASSWORD};
use datara_database::DatabaseDriver;
use datara_domain::{
    AuthenticationMode, ConnectionId, ConnectionProfile, Credentials, DomainError, EncryptionMode,
    SecretReference, Value,
};
use datara_driver_mssql::MssqlDriver;

#[tokio::test]
async fn connect_and_select_one() {
    let Some((_c, p, creds)) = mssql().await else {
        return;
    };
    let session = connect(&p, &creds).await.unwrap();
    let result = session
        .execute("master", "SELECT 1 AS one", 10)
        .await
        .unwrap();
    assert_eq!(result.columns[0].name, "one");
    assert_eq!(result.rows[0].cells[0], Value::Int(1));
}

#[tokio::test]
async fn list_databases_includes_master() {
    let Some((_c, p, creds)) = mssql().await else {
        return;
    };
    let session = connect(&p, &creds).await.unwrap();
    let dbs = session.list_databases().await.unwrap();
    assert!(dbs.iter().any(|d| d.name == "master"));
}

#[tokio::test]
async fn describe_table() {
    let Some((_c, p, creds)) = mssql().await else {
        return;
    };
    let session = connect(&p, &creds).await.unwrap();
    session
        .execute(
            "master",
            "CREATE TABLE it_t(id INT PRIMARY KEY, name NVARCHAR(50) NULL)",
            10,
        )
        .await
        .unwrap();
    let desc = session
        .describe_table("master", "dbo", "it_t")
        .await
        .unwrap();
    let id = desc.columns.iter().find(|c| c.name == "id").unwrap();
    assert!(id.is_primary_key);
    assert!(!id.nullable);
    let name = desc.columns.iter().find(|c| c.name == "name").unwrap();
    assert!(name.nullable);
    assert!(!name.is_primary_key);
}

#[tokio::test]
async fn write_query_returns_affected() {
    let Some((_c, p, creds)) = mssql().await else {
        return;
    };
    let session = connect(&p, &creds).await.unwrap();
    session
        .execute("master", "CREATE TABLE it_w(id INT)", 10)
        .await
        .unwrap();
    session
        .execute("master", "INSERT INTO it_w VALUES (1), (2)", 10)
        .await
        .unwrap();
    let result = session
        .execute("master", "UPDATE it_w SET id = id + 10", 10)
        .await
        .unwrap();
    assert_eq!(result.rows_affected, Some(2));
}

#[tokio::test]
async fn bad_password_is_authentication_error() {
    let Some((_c, p, _creds)) = mssql().await else {
        return;
    };
    let creds = Credentials {
        username: "sa".into(),
        password: "wrong-password".into(),
    };
    let err = MssqlDriver.connect(&p, &creds).await.err().unwrap();
    assert!(
        matches!(err, DomainError::Authentication { .. }),
        "expected Authentication, got {err:?}"
    );
}

#[tokio::test]
async fn tls_required_with_trust() {
    let Some((_c, mut p, creds)) = mssql().await else {
        return;
    };
    p.encryption = EncryptionMode::Required;
    p.trust_server_certificate = true;
    let session = connect(&p, &creds).await.unwrap();
    let result = session.execute("master", "SELECT 1", 10).await.unwrap();
    assert_eq!(result.rows[0].cells[0], Value::Int(1));
}

/// No container needed — the connect fails before the TDS handshake.
#[tokio::test]
async fn unreachable_host_is_connection_error() {
    let profile = ConnectionProfile {
        id: ConnectionId(0),
        name: "dead".into(),
        host: "127.0.0.1".into(),
        port: 1, // port 1 is never listening
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
    let err = MssqlDriver.connect(&profile, &creds).await.err().unwrap();
    assert!(
        matches!(err, DomainError::Connection { .. }),
        "expected Connection, got {err:?}"
    );
}
