use datara_domain::{AuthenticationMode, ConnectionId, EncryptionMode, QueryHistoryEntry};
use datara_storage::{NewConnection, Storage};
use tempfile::TempDir;

async fn open() -> (Storage, TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let storage = Storage::open(&dir.path().join("nested/test.db"))
        .await
        .unwrap();
    (storage, dir)
}

fn new_connection(name: &str) -> NewConnection {
    NewConnection {
        name: name.into(),
        host: "db.example.com".into(),
        port: 1433,
        database: Some("appdb".into()),
        username: "sa".into(),
        authentication: AuthenticationMode::SqlPassword,
        encryption: EncryptionMode::Required,
        trust_server_certificate: true,
    }
}

fn history_entry(connection_id: ConnectionId, query: &str, started_at: &str) -> QueryHistoryEntry {
    QueryHistoryEntry {
        id: 0, // assigned by the store
        connection_id,
        database: Some("appdb".into()),
        query: query.into(),
        started_at: started_at.parse().unwrap(),
        duration_ms: 42,
        row_count: 7,
        success: true,
        error_message: None,
    }
}

#[tokio::test]
async fn connection_round_trip() {
    let (storage, _dir) = open().await;
    let repo = storage.connections();

    // insert → secret_reference derived from the assigned id
    let id = repo.insert(new_connection("prod")).await.unwrap();
    assert!(id.0 > 0);
    let fetched = repo.get(id).await.unwrap();
    assert_eq!(fetched.id, id);
    assert_eq!(fetched.name, "prod");
    assert_eq!(fetched.port, 1433);
    assert_eq!(fetched.database.as_deref(), Some("appdb"));
    assert_eq!(fetched.encryption, EncryptionMode::Required);
    assert!(fetched.trust_server_certificate);
    assert_eq!(
        fetched.secret_reference.0,
        format!("mssql/{}/password", id.0)
    );

    // list
    repo.insert(new_connection("staging")).await.unwrap();
    let all = repo.list().await.unwrap();
    assert_eq!(all.len(), 2);
    assert_eq!(all[0].name, "prod");
    assert_eq!(all[1].name, "staging");

    // update
    let mut updated = fetched.clone();
    updated.name = "prod-renamed".into();
    updated.database = None;
    updated.encryption = EncryptionMode::Disabled;
    updated.trust_server_certificate = false;
    repo.update(&updated).await.unwrap();
    let after = repo.get(id).await.unwrap();
    assert_eq!(after.name, "prod-renamed");
    assert_eq!(after.database, None);
    assert_eq!(after.encryption, EncryptionMode::Disabled);
    assert!(!after.trust_server_certificate);
    // secret_reference is derived from the id and survives updates
    assert_eq!(after.secret_reference, fetched.secret_reference);

    // delete
    repo.delete(id).await.unwrap();
    assert!(repo.get(id).await.is_err());
    assert_eq!(repo.list().await.unwrap().len(), 1);
}

#[tokio::test]
async fn history_record_and_search() {
    let (storage, _dir) = open().await;
    let conn_a = storage
        .connections()
        .insert(new_connection("a"))
        .await
        .unwrap();
    let conn_b = storage
        .connections()
        .insert(new_connection("b"))
        .await
        .unwrap();
    let repo = storage.history();

    let id1 = repo
        .record(&history_entry(
            conn_a,
            "select * from users",
            "2026-09-27T10:00:00Z",
        ))
        .await
        .unwrap();
    let id2 = repo
        .record(&history_entry(
            conn_a,
            "select * from orders",
            "2026-09-27T11:00:00Z",
        ))
        .await
        .unwrap();
    repo.record(&history_entry(
        conn_b,
        "select * from users",
        "2026-09-27T12:00:00Z",
    ))
    .await
    .unwrap();
    assert!(id2 > id1);

    // LIKE filter across all connections, newest first
    let users = repo.search(None, "users", 10).await.unwrap();
    assert_eq!(users.len(), 2);
    assert_eq!(users[0].connection_id, conn_b); // started 12:00 > 10:00
    assert_eq!(users[1].connection_id, conn_a);

    // connection-scoped search
    let scoped = repo.search(Some(conn_a), "users", 10).await.unwrap();
    assert_eq!(scoped.len(), 1);
    assert_eq!(scoped[0].query, "select * from users");
    assert_eq!(scoped[0].duration_ms, 42);
    assert_eq!(scoped[0].row_count, 7);
    assert!(scoped[0].success);
    assert_eq!(
        scoped[0].started_at,
        "2026-09-27T10:00:00Z".parse().unwrap()
    );

    // limit
    assert_eq!(repo.search(None, "select", 1).await.unwrap().len(), 1);

    // no match
    assert!(repo
        .search(None, "drop table", 10)
        .await
        .unwrap()
        .is_empty());

    // delete
    repo.delete(id1).await.unwrap();
    assert_eq!(repo.search(None, "select", 10).await.unwrap().len(), 2);
}

#[tokio::test]
async fn history_failure_entry_round_trip() {
    let (storage, _dir) = open().await;
    let conn = storage
        .connections()
        .insert(new_connection("a"))
        .await
        .unwrap();
    let mut entry = history_entry(conn, "bad sql", "2026-09-27T10:00:00Z");
    entry.success = false;
    entry.error_message = Some("syntax error".into());
    entry.database = None;
    storage.history().record(&entry).await.unwrap();

    let found = storage.history().search(None, "bad", 10).await.unwrap();
    assert_eq!(found.len(), 1);
    assert!(!found[0].success);
    assert_eq!(found[0].error_message.as_deref(), Some("syntax error"));
    assert_eq!(found[0].database, None);
}

#[test]
fn history_entry_has_no_credential_field() {
    // The history record must never carry credential material. Assert on the
    // serialized field names so a future field addition fails this test.
    let entry = history_entry(ConnectionId(1), "select 1", "2026-09-27T10:00:00Z");
    let value = serde_json::to_value(&entry).unwrap();
    let keys: Vec<&String> = value.as_object().unwrap().keys().collect();
    assert!(
        !keys
            .iter()
            .any(|k| k.contains("password") || k.contains("secret")),
        "history entry must not carry credential fields: {keys:?}"
    );
}

/// The SQLite file holds profile metadata; `Storage::open` must pin it —
/// plus WAL/SHM sidecars, which carry the same data — to owner-only
/// `0600` regardless of umask.
#[cfg(unix)]
#[tokio::test]
async fn db_file_is_owner_only() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("app.db");

    // Seed a WAL-mode DB with live -wal/-shm sidecars via a separate
    // connection; it stays open so SQLite can't checkpoint the files
    // away before `Storage::open` tightens them.
    let wal_pool = sqlx::SqlitePool::connect_with(
        sqlx::sqlite::SqliteConnectOptions::new()
            .filename(&path)
            .create_if_missing(true)
            .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal),
    )
    .await
    .unwrap();
    sqlx::query("CREATE TABLE wal_probe (a INTEGER)")
        .execute(&wal_pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO wal_probe VALUES (1)")
        .execute(&wal_pool)
        .await
        .unwrap();

    let _storage = Storage::open(&path).await.unwrap();
    for suffix in ["", "-wal", "-shm"] {
        let p = format!("{}{}", path.display(), suffix);
        assert_eq!(
            std::fs::metadata(&p).unwrap().permissions().mode() & 0o777,
            0o600,
            "{p}"
        );
    }
}

/// `SavedRepo` round-trip: save → list → get → delete keeps name and query
/// text intact.
#[tokio::test]
async fn saved_query_round_trip() {
    let (storage, _dir) = open().await;
    let repo = storage.saved();

    let id = repo
        .save("daily", "select * from t\nwhere x = 1")
        .await
        .unwrap();
    assert!(id > 0);
    let other = repo.save("weekly", "select 42").await.unwrap();

    // list is name-ordered; multi-line text survives intact.
    let all = repo.list().await.unwrap();
    assert_eq!(all.len(), 2);
    assert_eq!(all[0].name, "daily");
    assert_eq!(all[0].query, "select * from t\nwhere x = 1");
    assert_eq!(all[1].id, other);

    let fetched = repo.get(id).await.unwrap();
    assert_eq!(
        (fetched.name.as_str(), fetched.query.as_str()),
        ("daily", "select * from t\nwhere x = 1")
    );

    repo.delete(id).await.unwrap();
    assert!(repo.get(id).await.is_err());
    assert_eq!(repo.list().await.unwrap().len(), 1);
}
