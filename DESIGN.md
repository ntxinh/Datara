# Datara — Design

Architecture of Datara as designed for the full MVP. **Status markers**
throughout: *(implemented)* means the code exists today; *(designed — phase
N)* means the contract is settled but the code does not. The design is fixed
by `docs/superpowers/specs/2026-09-27-datara-design.md`; this document
describes it for engineers.

## 1. System architecture

One binary, `datara`, with two entry modes: the Slint GUI (default) and a
stdio MCP server (`datara mcp-serve`). Both modes share the same database
service layer — the MCP server never duplicates driver logic.

```mermaid
flowchart TD
    GUI["Slint GUI<br/>(winit + Wayland)"] -->|bridges| SVC["DatabaseService<br/>(datara-database)"]
    MCP["MCP stdio server<br/>(rmcp)"] -->|reuses| SVC
    SVC --> DRV["DatabaseDriver trait"]
    DRV --> TDS["Tiberius<br/>TDS 7.3 + rustls"] --> MSSQL[(SQL Server)]
    SVC --> ST["Storage<br/>(SQLite / sqlx)"]
    SVC --> SEC["Secrets<br/>(Secret Service API)"]
    CFG["Config<br/>(XDG TOML)"] --> GUI
    CFG --> MCP
```

Dependency direction is strictly downward:

```
UI (Slint) → app → domain → infrastructure → drivers
```

- `datara-domain` never depends on Slint, sqlx, or Tiberius.
- `datara-app` is the only crate that depends on Slint.
- `datara-mcp-server` depends on `datara-database`, never on drivers directly.

## 2. Application architecture (workspace)

Ten crates under `crates/`, named `datara-<layer>`:

| Crate | Role | State |
|---|---|---|
| `app` | CLI (clap), Slint bridge, service wiring, `mcp-serve` entry | skeleton *(implemented: GUI launch, MCP stub exits 2)* |
| `domain` | `ConnectionProfile`, `QueryResult`, `Value`, `DomainError`, `Command` | *(implemented)* |
| `database` | `DatabaseDriver`/`DatabaseSession` traits, `DatabaseService` | traits *(implemented)*; service *designed — phase 2* |
| `driver-mssql` | Tiberius implementation of the traits | *designed — phase 2* |
| `sql-editor` | Statement detection (sqlparser), highlighting, completion | *designed — phase 4* |
| `data-grid` | `RowCache`, virtualized row model | *designed — phase 5* |
| `secrets` | Secret Service store | *designed — phase 2* |
| `mcp-server` | rmcp stdio server + tools | *designed — phase 7* |
| `storage` | SQLite repos: connections, history, saved queries | *(implemented)* |
| `config` | XDG paths (`AppPaths`), TOML settings (`AppConfig`) | *(implemented)* |

All third-party versions live only in the root `Cargo.toml`
(`[workspace.dependencies]`); member manifests use `{ workspace = true }`.

### Crate dependency graph

```mermaid
flowchart TD
    app --> domain
    app --> database
    app --> storage
    app --> config
    app --> secrets
    app --> sql_editor["sql-editor"]
    app --> data_grid["data-grid"]
    app --> mcp["mcp-server"]
    mcp --> database
    database --> domain
    driver["driver-mssql"] --> database
    driver --> domain
    storage --> domain
    secrets --> domain
    sql_editor --> domain
    data_grid --> domain
    config --> domain
```

`app` composes `driver-mssql` into `DatabaseService` at startup; nothing else
names the driver.

## 3. UI architecture

- Declarative `.slint` files under `ui/` (`app.slint`, `theme.slint`,
  `components/`, `pages/`, `dialogs/`, `editor/`, `grid/`); compiled by
  `slint-build` in `crates/app/build.rs` with the fluent style.
- Slint contains **view state only**. All actions cross a bridge in
  `crates/app/src/` (callbacks → commands → `DatabaseService` on Tokio).
- Keyboard-first: a `Command` enum (in `domain`) is the single source of
  shortcut bindings; components never hard-code keys.
- Currently: `ui/app.slint` is a single placeholder window proving the
  Wayland launch path *(implemented)*; `theme.slint` defines the
  dark/light palette *(implemented)*.

## 4. Database architecture

`DatabaseDriver::connect(profile, credentials) -> DatabaseSession`; sessions
expose `list_databases`, `list_schemas`, `list_tables`, `describe_table`,
`execute(database, query, max_rows)`, `cancel`, `quote_ident`. `list_*` takes
a database because MSSQL metadata requires a database context; `execute`
takes `max_rows` so drivers must cap materialization. Traits are
*(implemented)* in `datara-database`; the `DatabaseService` that owns a
session-per-connection map and enforces cancellation is *designed — phase 2*.

## 5. MSSQL driver

Tiberius 0.13 over TDS 7.3 with the rustls TLS backend. One Tokio TCP stream
per session; `cancel` issues an attention packet on the same connection.
`quote_ident` uses `[brackets]` with `]` escaped as `]]`. Value conversion
maps TDS types onto `domain::Value` (Null/Bool/Int/Float/Decimal/Text/Bytes/
DateTime/Uuid). *Designed — phase 2.*

## 6. SQL editor

`datara-sql-editor` provides pure functions: `detect_statement` (sqlparser-rs
with MSSQL dialect, byte-range output), tokenizer-based highlighting, and a
small completion provider. The Slint editor component calls `ExecuteQuery` on
Ctrl+Enter via the command system; execution never blocks the UI thread.
*Designed — phase 4.*

## 7. Data grid

`datara-data-grid` holds a `RowCache` keyed by row index plus a
`VirtualizedRows` model exposing only the viewport window to Slint — no
per-cell components. Target: responsive at tens of thousands of rows. Copy
cell/row/selection via clipboard; NULL renders as `NULL`. *Designed —
phase 5.*

## 8. Secrets

Passwords live only in the Secret Service API (GNOME Keyring; visible in
Seahorse). `ConnectionProfile` stores a `SecretReference` like
`mssql/7/password`; the actual secret is fetched at connect time and wrapped
in `secrecy::SecretString` so it cannot appear in `Debug`/`Display`/`serde`
output or error messages. *Designed — phase 2* (the `SecretReference` type is
implemented in domain).

## 9. TLS

`EncryptionMode::{Disabled, Preferred, Required}` maps onto Tiberius config.
Never silently downgraded: certificate errors surface to the user;
`trust_server_certificate` is a per-profile, user-visible boolean, not a
fallback. *Designed — phase 2.*

## 10. MCP server

`datara mcp-serve` runs an rmcp server over stdio only — no network
transport. Tools: `list_connections`, `list_databases`, `list_tables`,
`describe_table`, `search_schema`, `execute_query`. Every query tool takes an
explicit `connection_id` and `database`; results cap at
`mcp.max_result_rows` (default 1000) and report `truncated`. The server
resolves credentials the same way the GUI does and never returns secrets.
*Designed — phase 7.*

## 11. Local storage

SQLite via sqlx (bundled), at `~/.local/share/io.github.ntxinh.Datara/app.db`.
Tables: `connections` (metadata + `secret_reference`, no password column),
`query_history`, `saved_queries`. Migrations in
`crates/storage/migrations/`. *(Implemented)*.

## 12. Configuration

`~/.config/io.github.ntxinh.Datara/config.toml`, loaded with serde defaults —
missing file yields `AppConfig::default()`, malformed TOML errors naming the
path. Sections: `editor`, `query` (`default_limit = 1000`,
`timeout_seconds = 30`), `appearance`, `mcp` (`enabled = false`,
`max_result_rows = 1000`). `AppConfig::default_port() = 1433` is the single
source for the MSSQL port. XDG paths via `dirs`; no hard-coded `~`.
*(Implemented)*.

## 13. Errors

`DomainError` (thiserror) is the workspace error type: `Connection`,
`Authentication`, `Tls`, `Query{error_number,line,column}`, `Schema`,
`Storage`, `Secret`, `Config`, `Cancelled`, `Driver`. Variants carry
user-readable messages; SQL Server error numbers/positions are preserved for
the editor. `anyhow` is used only at binary boundaries (`main`). *(Domain
errors implemented.)*

## 14. Concurrency

One Tokio multi-thread runtime for the process. All database I/O runs on it;
the Slint UI thread never blocks — bridge calls spawn tasks and deliver
results back via `slint::invoke_from_event_loop`. Cancellation: each running
query holds a session handle; `cancel()` sends TDS attention. Async is used
only for real I/O. *Runtime wiring designed — phase 2.*

## 15. Performance

- Grid renders only the viewport; rows stream into `RowCache` incrementally.
- `execute` caps at `max_rows` at the driver, not the UI.
- Benchmarks (startup, schema load, result processing, grid scroll) via
  criterion — *phase 9*.

## 16. Security

Secrets-only-in-Secret-Service (§8), no silent TLS downgrade (§9), bounded
previews and results (§4, §10), no credential material in logs or errors,
MCP stdio-only with explicit connection scoping (§10). `cargo-audit` and
`cargo-deny` gate CI (`deny.toml` allowlist).

## 17. Packaging

- **RPM:** `packaging/rpm/datara.spec`, cargo build in `%build`, standard
  Fedora paths. *Designed — phase 8.*
- **Flatpak:** `io.github.ntxinh.Datara.yml` + vendored `cargo-sources.json`;
  minimal permissions — Wayland, Secret Service (session bus), network.
  *Designed — phase 8.*
- Desktop entry + AppStream metainfo in `packaging/share/`.
