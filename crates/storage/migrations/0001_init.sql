CREATE TABLE connections(
  id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL, host TEXT NOT NULL,
  port INTEGER NOT NULL DEFAULT 1433, database TEXT, username TEXT NOT NULL,
  auth_mode TEXT NOT NULL DEFAULT 'sql', encryption TEXT NOT NULL DEFAULT 'preferred',
  trust_server_certificate INTEGER NOT NULL DEFAULT 0,
  secret_reference TEXT NOT NULL, created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')));
CREATE TABLE query_history(
  id INTEGER PRIMARY KEY AUTOINCREMENT, connection_id INTEGER NOT NULL REFERENCES connections(id),
  database TEXT, query TEXT NOT NULL, started_at TEXT NOT NULL, duration_ms INTEGER NOT NULL,
  row_count INTEGER NOT NULL, success INTEGER NOT NULL, error_message TEXT);
CREATE TABLE saved_queries(id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL, query TEXT NOT NULL, created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')));
CREATE INDEX idx_history_conn ON query_history(connection_id, started_at DESC);
