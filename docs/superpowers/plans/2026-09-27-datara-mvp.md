# Datara MVP Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build Datara — a native Rust + Slint MSSQL database client for Fedora/Wayland — through 9 phased gates to a working MVP with MCP server, RPM/Flatpak packaging, and CI.

**Architecture:** Cargo workspace of focused crates (`UI → app → domain → infrastructure → drivers`); a Tokio `DatabaseService` shared by the Slint UI bridge and the rmcp stdio server; SQLite (sqlx) for local state; Secret Service for credentials; sqlparser-rs for statement detection and syntax highlighting.

**Tech Stack:** Rust 1.98.1 (mise), Slint 1.18 (winit + femtovg), Tokio 1.x, Tiberius 0.13, sqlx 0.9 (sqlite), secret-service 5.2, rmcp 3.4, sqlparser 0.63, clap 4.6, tracing, thiserror/anyhow, sqlx testcontainers-rs 0.15 over Podman.

**Spec:** `docs/superpowers/specs/2026-09-27-datara-design.md` (decisions) + `docs/superpowers/specs/2026-09-27-datara-build-spec.md` (authoritative requirements, verbatim).

## Global Constraints

Every task's requirements implicitly include:

- App ID `io.github.ntxinh.Datara`; binary name `datara`; license GPL-3.0; repo `github.com/ntxinh/Datara`.
- Rust `1.98.1` pinned in `mise.toml`; all commands via `make` targets or `mise exec --`.
- Crate names `datara-<layer>`; crate dirs `crates/<layer>`; workspace deps only in root `Cargo.toml`; member manifests inherit via `{ workspace = true }`. NEVER version numbers inside member manifests.
- Pinned versions: `slint=1.18`, `slint-build=1.18`, `tokio=1` (rt-multi-thread,macros,sync,time,fs), `tiberius=0.13`, `async-trait=0.1`, `sqlx=0.9` (runtime-tokio,sqlite,migrate), `secret-service=5` (rt-tokio-crypto-rustls), `secrecy=0.10`, `rmcp=3.4` (server,transport-io,macros), `schemars=1`, `sqlparser=0.63`, `serde=1`, `serde_json=1`, `toml=0.9` (config crate dep; `toml` file format crate is 1.x — use `toml="0.9"` for parsing), `clap=4` (derive), `tracing=0.1`, `tracing-subscriber=0.3` (env-filter), `thiserror=2`, `anyhow=1`, `dirs=7` (dev/test only), `rstest=0.27` (dev), `testcontainers=0.15` (dev), `testcontainers-modules=0.15` (dev), `tokio-stream=0.1`, `futures-core=0.3`, `uuid=1` (v4), `time`/`jiff` — use `jiff="0.2"` for timestamps in history.
- Non-negotiables: secrets only in Secret Service; never log credentials; never persist passwords in SQLite/TOML; no silent TLS downgrade; table preview `SELECT TOP N` (default 1000); UI thread never blocks; MCP stdio-only with explicit `connection_id`; XDG paths via `dirs`; `Config::default_port() = 1433` is the single source for the MSSQL default port.
- UI text/properties in English; no emoji in UI or docs.
- Every task ends green: `cargo fmt`, `cargo clippy -- -D warnings`, `cargo test`, `cargo build`. Commit per task with `feat:`/`fix:`/`docs:`/`chore:` conventional message.
- Never expose passwords in `Debug`/`Display`/`Serialize` impls or error messages.
- `// ponytail:` comment required on any deliberate simplification with a known ceiling.
- Test convention: unit tests beside code (`#[cfg(test)] mod tests`), integration in `crates/<x>/tests/` and top-level `tests/`. MSSQL integration tests use the shared harness in `tests/common/mod.rs` and are `#[ignore]`-free: they detect `DOCKER_HOST`/podman socket and `skip` (return early with eprintln) when unavailable rather than failing — EXCEPT the manual `make test-integration` run which is expected to run with podman socket active.

## File Map (authoritative)

```
Cargo.toml, mise.toml, .editorconfig, .gitignore, Makefile, deny.toml,
README.md, DESIGN.md, AGENTS.md, LICENSE (GPL-3.0)
.omp/lsp.json
.github/workflows/{ci,security,build,release}.yml
crates/app/{Cargo.toml,build.rs,src/{main.rs,ui.rs,bridge.rs,services.rs,cli.rs,
    commands.rs,schema_tree.rs,editor_ui.rs,history_ui.rs,mcp_main.rs}}
crates/domain/src/{lib.rs,connection.rs,types.rs,error.rs,command.rs,value.rs}
crates/database/src/{lib.rs,service.rs}
crates/driver-mssql/src/{lib.rs,driver.rs,session.rs,convert.rs,queries.rs}
crates/driver-mssql/tests/{common/mod.rs,mssql.rs}
crates/sql-editor/src/{lib.rs,statement.rs,highlight.rs,complete.rs}
crates/data-grid/src/{lib.rs,cache.rs,model.rs}
crates/secrets/src/{lib.rs,store.rs}
crates/mcp-server/src/{lib.rs,server.rs,tools.rs}
crates/storage/src/{lib.rs,db.rs,connections.rs,history.rs,migrations/0001_init.sql}
crates/config/src/{lib.rs,paths.rs,settings.rs}
ui/app.slint, ui/theme.slint, ui/components/{schema_tree,toolbar,status_bar,tabs}.slint,
ui/dialogs/connection.slint, ui/editor/query_editor.slint, ui/grid/result_grid.slint,
ui/pages/history.slint
assets/icons/datara.svg
packaging/rpm/datara.spec
packaging/flatpak/{io.github.ntxinh.Datara.yml,cargo-sources.json}
packaging/share/{io.github.ntxinh.Datara.desktop,io.github.ntxinh.Datara.metainfo.xml}
tests/README.md   (integration tests live in crates/*/tests/ so `cargo test --workspace` runs them)
docs/development/performance.md, docs/ui/components.md
docs/ (per spec §40 layout + docs/README.md index)
```

---

# PHASE 1 — Foundation

### Task 1.1: Workspace scaffold and toolchain files

**Files:**
- Create: `Cargo.toml`, `mise.toml`, `.editorconfig`, `.gitignore`, `Makefile`, `deny.toml`, `LICENSE`, `.omp/lsp.json`
- Create: `crates/{domain,database,driver-mssql,sql-editor,data-grid,secrets,mcp-server,storage,config,app}/Cargo.toml` + `crates/*/src/lib.rs` stubs (app gets `src/main.rs` + `build.rs` instead)

**Step 1: Write root files**

`Cargo.toml`:
```toml
[workspace]
resolver = "2"
members = [
    "crates/app", "crates/domain", "crates/database", "crates/driver-mssql",
    "crates/sql-editor", "crates/data-grid", "crates/secrets",
    "crates/mcp-server", "crates/storage", "crates/config",
]

[workspace.package]
version = "0.1.0"
edition = "2021"
license = "GPL-3.0-only"
repository = "https://github.com/ntxinh/Datara"
rust-version = "1.98"

[workspace.dependencies]
datara-domain = { path = "crates/domain" }
datara-database = { path = "crates/database" }
datara-driver-mssql = { path = "crates/driver-mssql" }
datara-sql-editor = { path = "crates/sql-editor" }
datara-data-grid = { path = "crates/data-grid" }
datara-secrets = { path = "crates/secrets" }
datara-mcp-server = { path = "crates/mcp-server" }
datara-storage = { path = "crates/storage" }
datara-config = { path = "crates/config" }
slint = { version = "1.18", default-features = false, features = ["backend-winit", "renderer-femtovg", "std"] }
slint-build = "1.18"
tokio = { version = "1", features = ["rt-multi-thread", "macros", "sync", "time", "fs"] }
tokio-stream = "0.1"
futures-core = "0.3"
futures-util = "0.3"
tiberius = { version = "0.13", default-features = false, features = ["tds73", "rustls"] }
async-trait = "0.1"
sqlx = { version = "0.9", default-features = false, features = ["runtime-tokio", "sqlite", "migrate"] }
secret-service = { version = "5", features = ["rt-tokio-crypto-rustls"] }
secrecy = { version = "0.10", features = ["serde"] }
rmcp = { version = "3.4", features = ["server", "transport-io", "macros"] }
schemars = "1"
sqlparser = "0.63"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
toml = "0.9"
clap = { version = "4", features = ["derive"] }
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
thiserror = "2"
anyhow = "1"
jiff = "0.2"
uuid = { version = "1", features = ["v4", "serde"] }
rstest = "0.27"
testcontainers = { version = "0.15", features = ["blocking"] }
testcontainers-modules = "0.15"
criterion = "0.8"
```

`mise.toml`:
```toml
[tools]
rust = "1.98.1"

[settings]
experimental = true
```

`.editorconfig`, `.gitignore`, `Makefile` (targets exactly per spec §44), `.omp/lsp.json`:
```json
{ "servers": { "rust-analyzer": {} } }
```

`deny.toml`: `[licenses] allow = ["GPL-3.0-only","GPL-3.0-or-later","MIT","MIT-0","Apache-2.0","Apache-2.0 WITH LLVM-exception","BSD-2-Clause","BSD-3-Clause","ISC","Zlib","Unicode-3.0","Unicode-DFS-2016","MPL-2.0","CC0-1.0","BSL-1.0","OFL-1.1"]`, `[advisories] yanked = "warn"`, `[bans] multiple-versions = "warn"`.

`LICENSE`: full GPL-3.0 text (`curl https://www.gnu.org/licenses/gpl-3.0.txt`).

Member manifests: `package.name = "datara-<x>"`, `package.workspace = true` fields; lib stubs `// Datara <x> crate`.

**Step 2:** `mise install && cargo generate-lockfile && cargo check --workspace` → workspace compiles (10 empty crates + app `fn main(){}`).

**Step 3: Commit** — `chore: workspace scaffold, toolchain pins, license`.

### Task 1.2: Domain types

**Files:** `crates/domain/Cargo.toml`, `src/lib.rs`, `src/connection.rs`, `src/types.rs`, `src/value.rs`, `src/error.rs`

**Produces** (consumed everywhere):
```rust
pub struct ConnectionId(pub i64);
pub struct SecretReference(pub String);          // e.g. "mssql/7/password"
pub enum AuthenticationMode { SqlPassword }
pub enum EncryptionMode { Disabled, Preferred, Required }
pub struct ConnectionProfile {
    pub id: ConnectionId, pub name: String, pub host: String, pub port: u16,
    pub database: Option<String>, pub username: String,
    pub authentication: AuthenticationMode, pub encryption: EncryptionMode,
    pub trust_server_certificate: bool, pub secret_reference: SecretReference,
}
pub struct Credentials { pub username: String, pub password: secrecy::SecretString }
pub struct DatabaseInfo { pub name: String }
pub struct SchemaInfo { pub name: String }
pub struct TableInfo { pub schema: String, pub name: String, pub kind: TableKind } // Base|View
pub struct ColumnInfo { pub name: String, pub data_type: String, pub nullable: bool, pub is_primary_key: bool, pub ordinal: u32 }
pub struct TableDescription { pub schema: String, pub name: String, pub columns: Vec<ColumnInfo>, pub indexes: Vec<IndexInfo> }
pub struct IndexInfo { pub name: String, pub columns: Vec<String>, pub is_unique: bool }
pub struct QueryResult { pub columns: Vec<QueryColumn>, pub rows: Vec<QueryRow>, pub rows_affected: Option<u64>, pub truncated: bool }
pub struct QueryColumn { pub name: String, pub data_type: String }
pub struct QueryRow { pub cells: Vec<Value> }
pub enum Value { Null, Bool(bool), Int(i64), Float(f64), Decimal(String), Text(String), Bytes(Vec<u8>), DateTime(String), Uuid(uuid::Uuid) }
pub struct QueryHistoryEntry { pub id: i64, pub connection_id: ConnectionId, pub database: Option<String>, pub query: String, pub started_at: jiff::Timestamp, pub duration_ms: u64, pub row_count: u64, pub success: bool, pub error_message: Option<String> }
pub struct SavedQuery { pub id: i64, pub name: String, pub query: String, pub created_at: jiff::Timestamp }
pub enum Command { ExecuteQuery, NewQuery, SaveQuery, Search, OpenPalette, Find, SearchHistory, NextTab, CloseTab, OpenConnection, RefreshSchema, ToggleSidebar }
```

`Display` for `Value` (NULL → `"NULL"`, Text raw, etc.). `DomainError` in `error.rs`: `Connection{message}`, `Authentication{message}`, `Tls{message}`, `Query{message, error_number:Option<i32>, line:Option<u32>, column:Option<u32>}`, `Schema{message}`, `Storage{message}`, `Secret{message}`, `Config{message}`, `Cancelled`, `Driver{message}` — `thiserror`, user-readable `Display` (§27 style). `type Result<T> = std::result::Result<T, DomainError>`.

**Steps:** write tests for `Value::Display` (Null/Bool/Int/Text), `DomainError` display strings, `SecretReference` serde. Implement, `cargo test -p datara-domain`, commit `feat(domain): core types and errors`.

### Task 1.3: Config + XDG paths

**Files:** `crates/config/src/{lib,paths,settings}.rs`

**Produces:**
```rust
pub struct AppPaths { data_dir: PathBuf, config_dir: PathBuf, state_dir: PathBuf }
impl AppPaths { pub fn new() -> Result<Self>; pub fn app_db(&self) -> PathBuf /* data/app.db */; pub fn config_file(&self) -> PathBuf; pub fn log_dir(&self) -> PathBuf; }
pub struct AppConfig { pub editor: EditorConfig, pub query: QueryConfig, pub appearance: AppearanceConfig, pub mcp: McpConfig }
// EditorConfig{font_size=14,tab_size=4} QueryConfig{default_limit=1000,timeout_seconds=30}
// AppearanceConfig{theme="system"} McpConfig{enabled=false,max_result_rows=1000}
impl AppConfig { pub fn load(paths:&AppPaths)->Result<Self>; pub fn default_port()->u16 {1433} }
```
XDG via `dirs::{data_dir,config_dir,state_dir}` + `io.github.ntxinh.Datara`; create dirs on `new()`. TOML load: missing file → defaults; malformed → `Config` error naming the file. Serde defaults on every field.

**Steps:** rstest tests (defaults round-trip, missing file → defaults, partial TOML keeps unspecified defaults, bad TOML errors with path). `cargo test -p datara-config`. Commit `feat(config): XDG paths and TOML settings`.

### Task 1.4: Database abstraction traits + Command

**Files:** `crates/database/src/lib.rs`

**Produces:**
```rust
#[async_trait] pub trait DatabaseDriver: Send + Sync {
    async fn connect(&self, profile:&ConnectionProfile, credentials:&Credentials) -> Result<Box<dyn DatabaseSession>>;
}
#[async_trait] pub trait DatabaseSession: Send {
    async fn list_databases(&self) -> Result<Vec<DatabaseInfo>>;
    async fn list_schemas(&self, database:&str) -> Result<Vec<SchemaInfo>>;
    async fn list_tables(&self, database:&str, schema:&str) -> Result<Vec<TableInfo>>;
    async fn describe_table(&self, database:&str, schema:&str, table:&str) -> Result<TableDescription>;
    async fn execute(&self, database:&str, query:&str, max_rows:usize) -> Result<QueryResult>;
    async fn cancel(&self) -> Result<()>;
    fn quote_ident(&self, ident:&str) -> String; // MSSQL: [brackets], escape ]
}
```
Design note for file header comment: `list_*` per database because MSSQL requires a database context; `execute` takes `max_rows` so drivers must cap materialization (spec §11).

**Steps:** compile gate only + a doc test on `quote_ident` shape. Commit `feat(database): driver/session traits`.

### Task 1.5: Storage — SQLite schema + repositories

**Files:** `crates/storage/src/{lib,db,connections,history}.rs`, `crates/storage/migrations/0001_init.sql`

Migration SQL:
```sql
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
```

**Produces:**
```rust
pub struct Storage { pool: sqlx::SqlitePool }
impl Storage { pub async fn open(path:&Path)->Result<Self>; } // create dirs, run migrations
pub struct ConnectionRepo<'a>(&'a Storage);
impl ConnectionRepo<'_> { pub async fn list(&self)->Result<Vec<ConnectionProfile>>;
  pub async fn get(&self,id:ConnectionId)->Result<ConnectionProfile>;
  pub async fn insert(&self,NewConnection)->Result<ConnectionId>; // NewConnection = profile minus id, secret_reference auto = format!("mssql/{id}/password") generated AFTER insert via last_insert_id then UPDATE — simpler: insert with placeholder, update row with computed ref
  pub async fn update(&self,&ConnectionProfile)->Result<()>;
  pub async fn delete(&self,ConnectionId)->Result<()>; }
pub struct HistoryRepo<'a>(&'a Storage);
impl HistoryRepo<'_> { pub async fn record(&self,&QueryHistoryEntry)->Result<i64>;
  pub async fn search(&self,connection_id:Option<ConnectionId>,filter:&str,limit:u32)->Result<Vec<QueryHistoryEntry>>; // LIKE '%f%' on query, ordered started_at DESC
  pub async fn delete(&self,id:i64)->Result<()>; }
```
Timestamps stored RFC3339 via `jiff`. `NewConnection` type exported.

**Steps:** tests with `sqlx::SqlitePool` on tempfile: insert→list→get→update→delete profile round-trip (assert `secret_reference` format), history record+search LIKE match, history row has no password field (assert struct fields). `cargo test -p datara-storage`. Commit `feat(storage): SQLite schema and repos`.

### Task 1.6: Minimal Slint window launches on Wayland

**Files:** `crates/app/Cargo.toml`, `crates/app/build.rs`, `crates/app/src/main.rs`, `ui/app.slint`, `ui/theme.slint`

`build.rs`: `fn main(){ slint_build::compile_with_config("../../ui/app.slint", slint_build::CompilerConfiguration::new().with_style("fluent".into())).unwrap(); }`

`ui/theme.slint`: `global Theme { out property<bool> dark: true; out property<color> bg: dark?#1e1e2e:#fafafa; ... fg, accent #89b4fa, border, panel }` — Catppuccin-ish palette, both themes defined, `dark` bound to Palette later.

`ui/app.slint` (Task-1 version, grows later):
```slint
import { Theme } from "theme.slint";
export component MainWindow inherits Window {
    title: "Datara";
    preferred-width: 1100px; preferred-height: 700px;
    background: Theme.bg;
    VerticalLayout { padding: 16px;
        Text { text: "Datara"; font-size: 28px; color: Theme.fg; }
        Text { text: "MSSQL client — foundation build"; color: Theme.fg; opacity: 0.6; }
    }
}
```

`main.rs`: clap `Cli { command: Option<Cmd> }`, `Cmd::{Gui, McpServe}` (default Gui). Gui path: init tracing (`EnvFilter`, default `info,datara=debug`), `slint::init_translations!` n/a; `MainWindow::new()?.run()?`. Set app_id via `winit` platform — Slint winit backend reads `WAYLAND_DISPLAY` automatically; add `std::env::set_var("WINIT_UNIX_BACKEND","wayland")` when `WAYLAND_DISPLAY` is set AND user hasn't overridden, comment explaining why (native Wayland, no XWayland). Application ID: Slint uses the window title/class — set via `MainWindow` title + `.desktop` file `StartupWMClass`; add `slint::BackendSelector`/`winit` app-id hook if supported in 1.18 (`slint::platform` hook) — implementer verifies: run `xprop`-equivalent under Wayland via `swaymsg`/`niri msg` is N/A; instead verify `WAYLAND_DISPLAY` used + log backend. Acceptance: window renders, close works, log shows startup.

Steps: `cargo build -p datara`; smoke: `cargo run -p datara` under current Wayland session — window appears. Screenshot not scriptable; verify exit code + log line + `WAYLAND_DISPLAY` honored (winit logs `wayland` when `RUST_LOG=winit=debug`). Commit `feat(app): minimal Slint Wayland window`.

### Task 1.7: Docs skeleton + AGENTS.md + DESIGN.md + README.md

**Files:** `README.md`, `DESIGN.md`, `AGENTS.md`, `docs/README.md`, `docs/architecture/{overview,application,database,ui}.md`, `docs/development/{setup,testing,debugging}.md`, `docs/database/{mssql,query-execution,schema-browser}.md`, `docs/mcp/{overview,tools,security}.md`, `docs/security/{secrets,tls}.md`, `docs/packaging/{rpm,flatpak}.md`, `docs/decisions/ADR-{001,002,003}-*.md`, `CONTRIBUTING.md`

Content rules: README concise per §37 (project, platform, MSSQL, features list with status checkboxes, dev setup `mise install && make build`, security note, status=early). DESIGN.md = the mermaid architecture (spec §38 diagram + crate dependency graph + concurrency model + error/secret flows). AGENTS.md per §39 with REAL commands (`make build/test/lint`, `mise install`) and REAL rules from spec §32+§41 (no business logic in Slint, no DB from UI, MCP reuses services, no passwords in SQLite, no Postgres/SQLite drivers yet, doc-sync workflow). ADR-001 Rust+Slint (+GPL-3.0 rationale), ADR-002 MSSQL-first, ADR-003 MCP-stdio. Other docs: 5-15 line stubs describing the subsystem's current state ("implemented: X; pending: phase N"), never placeholder text like TBD — if nothing exists yet, describe the contract from this plan and mark status `planned`.

Steps: docs written, `make docs-check` prints index integrity (simple Makefile target: verify every file in docs/README.md exists — implement as a 6-line shell loop), commit `docs: documentation skeleton`.

### Task 1.8: GitHub Actions workflows

**Files:** `.github/workflows/{ci,security,build,release}.yml`

ci.yml: on push/PR — `jdx/mise-action@v3` (verify tag exists; else pin `jdx/mise-action@v2`), `apt install pkg-config libfontconfig-dev libxkbcommon-dev libwayland-dev libgl1-mesa-dev`, steps: fmt --check → check → clippy `-D warnings` → test --workspace → build --release. Cache: `Swatinem/rust-cache@v2`.

security.yml: schedule weekly + push — `cargo audit` (rustsec `audit-check` or cargo-install audit), `cargo deny check` (installed via `cargo-binstall` or mise).

build.yml: on push — release build artifact upload (tarball `datara-x86_64-unknown-linux-gnu.tar.gz` containing binary + desktop file + icon).

release.yml: on tag `v*` — build release, create GH release with tarball (RPM/Flatpak are phase 8; workflow gains rpm step then — plan for a `package` job placeholder implementing tarball now, RPM added in Task 8.2 by editing this file).

Steps: `actionlint`-style YAML sanity (run `python3 -c "import yaml; yaml.safe_load(...)"` per file — pyyaml may be absent; use `ruby -ryaml` or cargo — simplest: `npx`? use python3 fallback). Commit `ci: GitHub Actions workflows`.

**Phase 1 gate:** `make check lint test build` green; `cargo run -p datara` shows window on Wayland; CI files parse; docs tree exists.

---

# PHASE 2 — Database (secrets, MSSQL driver, service layer, SELECT 1)

### Task 2.1: Secret Service store

**Files:** `crates/secrets/src/lib.rs`, `crates/secrets/src/store.rs`

**Produces:**
```rust
pub struct SecretStore { ss: secret_service::SecretService<'static> }
impl SecretStore {
    pub async fn connect() -> Result<Self>;            // EncryptionType::Dh; unlock default collection if locked
    pub async fn save(&self, reference:&SecretReference, label:&str, secret:&SecretString) -> Result<()>;
    pub async fn load(&self, reference:&SecretReference) -> Result<SecretString>;       // Secret error "not found" if absent
    pub async fn delete(&self, reference:&SecretReference) -> Result<()>;
}
```
Attributes: `{"application": "io.github.ntxinh.Datara", "secret-reference": <ref>}`. `save` = `create_item(label, attrs, secret_bytes, replace=true, "text/plain")`. `load` = `search_items` → first unlocked → `get_secret`. `delete` = search → `item.delete()`. Map every error to `DomainError::Secret`; error strings name the action, never the secret.

**Steps:** doc-comment the D-Bus requirement; unit test only the attribute map builder + error mapping (live Secret Service test gated behind `DATARA_LIVE_SECRETS=1` env — write it, it skips otherwise). `cargo test -p datara-secrets`. Commit `feat(secrets): Secret Service store`.

### Task 2.2: MSSQL driver

**Files:** `crates/driver-mssql/src/{lib,driver,session,convert,queries}.rs`

`queries.rs` — constants:
```rust
pub const LIST_DATABASES: &str = "SELECT name FROM sys.databases ORDER BY name";
pub const LIST_SCHEMAS: &str = "SELECT name FROM sys.schemas ORDER BY name";
pub const LIST_TABLES: &str = "SELECT s.name AS schema_name, o.name, CASE o.type WHEN 'V' THEN 'view' ELSE 'table' END AS kind FROM sys.objects o JOIN sys.schemas s ON o.schema_id = s.schema_id WHERE o.type IN ('U','V') AND s.name = @P1 ORDER BY o.name";
pub const DESCRIBE_COLUMNS: &str = "SELECT c.name, ty.name AS data_type, c.is_nullable, CASE WHEN pk.column_id IS NULL THEN 0 ELSE 1 END AS is_pk, c.column_id AS ordinal FROM sys.columns c JOIN sys.types ty ON c.user_type_id = ty.user_type_id JOIN sys.objects o ON c.object_id = o.object_id JOIN sys.schemas s ON o.schema_id = s.schema_id LEFT JOIN (SELECT ic.object_id, ic.column_id FROM sys.indexes i JOIN sys.index_columns ic ON i.object_id = ic.object_id AND i.index_id = ic.index_id WHERE i.is_primary_key = 1) pk ON pk.object_id = c.object_id AND pk.column_id = c.column_id WHERE s.name = @P1 AND o.name = @P2 ORDER BY c.column_id";
pub const DESCRIBE_INDEXES: &str = "SELECT i.name, i.is_unique, c.name AS column_name FROM sys.indexes i JOIN sys.index_columns ic ON i.object_id = ic.object_id AND i.index_id = ic.index_id JOIN sys.columns c ON ic.object_id = c.object_id AND ic.column_id = c.column_id JOIN sys.objects o ON i.object_id = o.object_id JOIN sys.schemas s ON o.schema_id = s.schema_id WHERE s.name = @P1 AND o.name = @P2 AND i.is_primary_key = 0 AND i.name IS NOT NULL ORDER BY i.name, ic.key_ordinal";
```

`driver.rs`:
```rust
pub struct MssqlDriver;
#[async_trait] impl DatabaseDriver for MssqlDriver {
    async fn connect(&self, profile:&ConnectionProfile, creds:&Credentials) -> Result<Box<dyn DatabaseSession>> {
        let mut config = tiberius::Config::new();
        config.host(&profile.host); config.port(profile.port);
        if let Some(db) = &profile.database { config.database(db); }
        config.authentication(AuthMethod::sql_server(&creds.username, creds.password.expose_secret()));
        config.encryption(match profile.encryption {
            EncryptionMode::Disabled => EncryptionLevel::Off,
            EncryptionMode::Preferred => EncryptionLevel::On,
            EncryptionMode::Required => EncryptionLevel::Required,
        });
        if profile.trust_server_certificate { config.trust_cert(); }
        let tcp = tokio::time::timeout(
            std::time::Duration::from_secs(10),               // connection timeout
            TcpStream::connect((profile.host.as_str(), profile.port))).await
            .map_err(|_| DomainError::Connection{ message: format!("connection to {}:{} timed out after 10s", profile.host, profile.port) })?
            .map_err(|e| DomainError::Connection{ message: format!("could not connect to {}:{} — {e}", profile.host, profile.port) })?;
        tcp.set_nodelay(true).ok();
        let client = Client::connect(config, tcp.compat_write()).await.map_err(map_tiberius_error)?;
        Ok(Box::new(MssqlSession { client: Mutex::new(client), database: profile.database.clone() }))
    }
}
```
(tiberius dep: `{ version="0.13", features=["tds73","rustls","time"] }`; add `tokio-util` `{features=["compat"]}` + `time="0.3"` to workspace deps.)

`map_tiberius_error(e:tiberius::error::Error)->DomainError`: `Error::Server(tok)` with code 18456/18452/18453 → `Authentication`; `Error::Tls{..}`/`Error::Rustls`→`Tls{message}`; `Error::Io{..}`→`Connection`; other `Server(tok)`→`Query{message:tok.message(), error_number:Some(tok.code() as i32), line, column}`; else `Driver`.

`session.rs`: `MssqlSession { client: Mutex<Client<Compat<TcpStream>>>, database: Option<String> }`. Implements `DatabaseSession`:
- `list_databases`: `client.simple_query(LIST_DATABASES)` → rows → `DatabaseInfo{name}`.
- `list_schemas(database)`: `use_db` first — MSSQL requires `USE [db]`; session holds `switch_db(database)` helper: `client.execute(format!("USE {}", quote_ident(database)),&[])`.
- `list_tables`, `describe_table` (two queries → TableDescription{indexes grouped}), `execute(database,query,max_rows)`:
  - `switch_db(database)` if Some.
  - classify via `sqlparser::Parser::parse_sql(&GenericDialect{}, query)` → `Ok(stmts)` all `Statement::Query` → row-returning path `client.query`; else `client.execute` → `rows_affected=Some(total)`, empty columns/rows. Parse error → `client.query` path (server decides; never let the parser block valid SQL).
  - Row path: `stream.try_next()` loop on `QueryItem::{Metadata(meta) → columns from meta.columns() (name + format!("{:?}", column_type()) — ponytail: debug name for type, map to friendly names when type display matters), Row(row) → convert each ColumnData via `column_data_to_value`; if rows.len()==max_rows { truncated=true; keep draining but discard — keeps TDS protocol clean (attention cancel is a later upgrade) }`.
- `quote_ident`: `"[" + ident.replace(']', "]]") + "]"`.
- `cancel`: `Ok(())` — real cancellation = aborting the service-layer task, which drops the TCP stream (documented in service).

`convert.rs`: `fn column_data_to_value(d:&tiberius::ColumnData)->Value` — match: `ColumnData::U8/I16/I32/I64/Bit/F32/F64`→ Int/Float/Bool; `String`→Text; `Guid`→Uuid; `Numeric`/`Decimal`→`Decimal(n.to_string())`; `Binary`→Bytes; `DateTime/DateTime2/Date/Time/DateTimeOffset` (with `time` feature types) → `DateTime(formatted)`; `Xml`→Text; `_`→`Text(format!("{:?}",d))` ponytail fallback.

**Produces:** `MssqlDriver` (unit-type, `Clone`/`Default`), `MssqlSession`, `map_tiberius_error`, `column_data_to_value`, query constants.

**Steps:** unit tests — `quote_ident` (`a`→`[a]`, `a]b`→`[a]]b]`), `column_data_to_value` for Int/Bit/String/Null, `map_tiberius_error` code 18456 → Authentication variant. `cargo test -p datara-driver-mssql`. Commit `feat(driver-mssql): Tiberius driver`.

### Task 2.3: DatabaseService + testcontainers harness

**Files:** `crates/database/src/{lib,service}.rs`, `crates/driver-mssql/tests/common/mod.rs`, `crates/driver-mssql/tests/mssql.rs`

> Deviation note (vs spec file map): workspace `cargo test --workspace` only runs member-package tests, so the shared container harness lives in `crates/driver-mssql/tests/common/` and other crates include it via `#[path]`. Top-level `tests/` stays for spec layout parity with a README pointer.

```rust
// service.rs
pub struct DatabaseService {
    storage: Arc<Storage>, secrets: Arc<SecretStore>,
    driver: MssqlDriver,
    sessions: tokio::sync::Mutex<HashMap<ConnectionId, Arc<dyn DatabaseSession>>>,
}
impl DatabaseService {
    pub fn new(storage: Arc<Storage>, secrets: Arc<SecretStore>) -> Self;
    pub async fn test_connection(&self, profile:&ConnectionProfile, creds:&Credentials) -> Result<()>; // connect + drop, nothing cached
    pub async fn session(&self, id:ConnectionId) -> Result<Arc<dyn DatabaseSession>>; // get-or-connect: profile+password from storage/secrets
    pub async fn disconnect(&self, id:ConnectionId);
    pub async fn list_databases(&self,id:ConnectionId)->Result<Vec<DatabaseInfo>>;
    pub async fn list_schemas(&self,id:&ConnectionId,database:&str)->Result<Vec<SchemaInfo>>;
    pub async fn list_tables(&self,id:&ConnectionId,database:&str,schema:&str)->Result<Vec<TableInfo>>;
    pub async fn describe_table(&self,...)->Result<TableDescription>;
    pub async fn execute(&self,id:ConnectionId,database:Option<String>,query:String,max_rows:usize)
        -> Result<QueryHandle>;
    // QueryHandle = JoinHandle<Result<QueryResult>>; caller aborts to cancel (aborting drops the
    // TCP stream → server aborts the batch). Records QueryHistoryEntry on completion — inside the
    // spawned task, via HistoryRepo (owns duration/row_count/success).
}
```
`session()` concurrency: `Mutex<HashMap>` + per-session `Arc`; connect outside the map lock held only for lookup/insert (use entry-point `get` then `lock().insert` — two concurrent connects for same id: last wins, benign — ponytail comment).

`tests/common/mod.rs`:
```rust
use testcontainers::{core::WaitFor, runners::AsyncRunner, ContainerAsync, GenericImage, ImageExt};
pub const SA_PASSWORD: &str = "Str0ng!Passw0rd";
pub fn ensure_podman() -> bool {
    if std::env::var("DOCKER_HOST").is_err() {
        // podman rootless socket lives at $XDG_RUNTIME_DIR/podman/podman.sock
        let Ok(dir) = std::env::var("XDG_RUNTIME_DIR") else { return false };
        let sock = format!("{dir}/podman/podman.sock");
        if !std::path::Path::new(&sock).exists() { return false; }
        std::env::set_var("DOCKER_HOST", format!("unix://{sock}"));
    }
    true
}
pub async fn mssql() -> Option<(ContainerAsync<GenericImage>, ConnectionProfile, Credentials)> {
    if !ensure_podman() { eprintln!("SKIP: no podman socket"); return None; }
    let image = GenericImage::new("mcr.microsoft.com/mssql/server", "2022-latest")
        .with_env_var("ACCEPT_EULA","Y").with_env_var("MSSQL_SA_PASSWORD", SA_PASSWORD)
        .with_exposed_port(1433.tcp())
        .with_wait_for(WaitFor::message_on_stdout("SQL Server is now ready for client connections"));
    let container = image.start().await.ok()?;
    let port = container.get_host_port_ipv4(1433).await.ok()?;
    let profile = ConnectionProfile{ id:ConnectionId(0), name:"it".into(), host:"127.0.0.1".into(), port,
        database:Some("master".into()), username:"sa".into(), authentication:AuthenticationMode::SqlPassword,
        encryption:EncryptionMode::Preferred, trust_server_certificate:true,
        secret_reference:SecretReference("test".into()) };
    let creds = Credentials{ username:"sa".into(), password:SA_PASSWORD.into() };
    Some((container, profile, creds))
}
```
Add `testcontainers`/`testcontainers-modules` dev-deps to driver-mssql (GenericImage is in `testcontainers` core, not modules).

`tests/mssql.rs` — each test `let Some((c,p,creds)) = mssql().await else { return }`:
1. `connect_and_select_one` → execute `SELECT 1 AS one` → `Value::Int(1)`, columns[0].name=="one".
2. `list_databases_includes_master`.
3. `describe_table`: execute DDL `CREATE TABLE it_t(id INT PRIMARY KEY, name NVARCHAR(50) NULL);` then describe → columns assert `id` is_pk, `name` nullable.
4. `write_query_returns_affected`: `UPDATE` on temp → `rows_affected=Some(n)`.
5. `bad_password_is_authentication_error` → `DomainError::Authentication`.
6. `tls_required_with_trust`: `EncryptionMode::Required` + `trust_server_certificate:true` connects.
7. `unreachable_host_is_connection_error` → port 1 → `DomainError::Connection`.

**Steps:** `cargo test -p datara-driver-mssql` (live tests run under podman; ~1–2 min image pull first time). Commit `feat(database): service layer + MSSQL integration tests`.

### Task 2.4: Connection dialog + sidebar + bridge (SELECT 1 end-to-end)

**Files:** `crates/app/src/{ui,bridge,services}.rs`, `ui/app.slint`, `ui/dialogs/connection.slint`, `ui/components/sidebar.slint`

**Architecture (locks in for all later phases):**
```rust
// bridge.rs — Slint ↔ Tokio boundary
pub enum AppEvent {                       // core → UI (invoke_from_event_loop)
    ConnectionsLoaded(Vec<ConnectionProfile>),
    Status(String),
    ConnectTestResult(Result<(), String>), // Ok(()) or error string
    Connected(ConnectionId),
    ConnectFailed { id: ConnectionId, message: String },
}
pub struct UiHandle { weak: slint::Weak<MainWindow> }  // .dispatch(AppEvent) → set props/callbacks
pub fn spawn_runtime() -> tokio::runtime::Runtime      // multi_thread, named "datara-db"
```
`main.rs`: build `Runtime` + `Storage::open` + `SecretStore::connect` + `DatabaseService` → `Arc` → `services::AppServices`. `ui.rs` wires `Bridge` callbacks → `runtime.spawn`, events → `UiHandle.dispatch` → update Slint models.

`ui/dialogs/connection.slint`: `ConnDialog inherits Dialog` — `ConnForm` struct fields (name, host, port, database, username, password, encryption∈{"disabled","preferred","required"}, trust-cert); buttons Test/Save/Cancel; `callback accepted(ConnForm)`, `callback test(ConnForm)`; `in property<string> test-result`. Port `SpinBox`/`LineEdit` validated 1–65535.

Sidebar: `repeater` over `Bridge.connections` → name + "Connect" button → `Bridge.connect-profile(id)`.

`Bridge` global (initial version — grows later):
```slint
export struct ConnForm { name:string, host:string, port:int, database:string, username:string, password:string, encryption:string, trust-cert:bool }
export struct ConnectionItem { id:int, name:string, host:string }
export global Bridge {
    callback new-connection(ConnForm);
    callback test-connection(ConnForm);
    callback connect-profile(int);
    in property<[ConnectionItem]> connections;
    in property<string> status;
    in property<string> test-result;
}
```

Flow: new-connection → `ConnectionRepo::insert` → `secrets.save(ref, password)` → reload list. test → `service.test_connection`. connect → `service.session(id)` → `Connected` event → status "Connected to <name>".

**Steps:** build; smoke `cargo run` — dialog opens, form validates, save persists (verify row via `sqlite3 ~/.local/share/io.github.ntxinh.Datara/app.db 'select * from connections'` — no password column); live Secret Service save → `secret-tool search application io.github.ntxinh.Datara` shows item (if secret-tool present; else Seahorse). Commit `feat(app): connection dialog, Secret Service wiring`.

**Phase 2 gate:** `SELECT 1` executes against containerized SQL Server via `DatabaseService::execute` (asserted in Task 2.3 test 1), connection dialog persists profile + password in Secret Service.

---

# PHASE 3 — Schema explorer

### Task 3.1: Flat tree model + schema loading

**Files:** `crates/app/src/schema_tree.rs`, `ui/components/sidebar.slint`

**Produces:**
```rust
pub enum NodeKind { Connection, Database, Folder /*Tables|Views*/, Table, View, Column }
pub struct TreeNode { pub id: i32, pub depth: u16, pub kind: NodeKind, pub label: String,
    pub has_children: bool, pub expanded: bool, // nav payload:
    pub connection_id: Option<ConnectionId>, pub database: Option<String>, pub schema: Option<String>, pub table: Option<String> }
pub struct SchemaTree { nodes: Vec<TreeNode>, next_id: i32 }
impl SchemaTree {
    pub fn set_connections(&mut self, conns: Vec<ConnectionProfile>);
    pub fn expandable(&self, idx: usize) -> Option<&TreeNode>;            // node needing load
    pub fn expand_placeholder(&mut self, idx: usize);                     // mark expanded + "Loading…" child
    pub fn replace_children(&mut self, parent_id: i32, children: Vec<TreeNode>);
    pub fn collapse(&mut self, idx: usize);
    pub fn slint_model(&self) -> Vec<SlintTreeNode>;                      // flat visible rows
}
```
Expansion semantics: Connection→databases; Database→folder "Tables" + "Views"; folder→`list_tables` filtered by kind; Table→columns via `describe_table`. Lazy: only fetch on expand; `RefreshSchema` re-fetches expanded nodes.

Slint side: `TreeNode` struct {id:int,depth:int,kind:string,label:string,has-children:bool,expanded:bool}; row delegate = indent(depth*16px) + chevron + icon-by-kind + label; click → `Bridge.toggle-node(id)`; double-click table → `Bridge.open-table(id)` (Phase 3.2).

**Steps:** unit tests on `SchemaTree` (expand placeholder → replace children → collapse removes subtree; depths correct; visible model excludes collapsed children). Commit `feat(app): schema tree model`.

### Task 3.2: Wire tree to DatabaseService + table preview

**Files:** `crates/app/src/bridge.rs`, `crates/app/src/ui.rs`

`toggle-node` → if needs load → `runtime.spawn(service.list_*)` → event `TreeUpdated(Vec<TreeNode>)` → UI replaces model. `open-table(node)` → status event `OpenTable{conn_id,database,schema,table}` — consumed by Phase 5 grid; for now Task 3.2 generates the preview SQL `SELECT TOP {limit} * FROM {schema}.{table}` (both via `quote_ident`) and emits `Status` showing it — editor wiring lands in Phase 4.

**Steps:** unit test `preview_sql(schema,table,limit)` string builder (quoting, limit substitution). Build + smoke expand against container. Commit `feat(app): schema loading and preview SQL`.

---

# PHASE 4 — SQL editor

### Task 4.1: Statement detection + tokenizer highlight

**Files:** `crates/sql-editor/src/{lib,statement,highlight,complete}.rs`

**Produces:**
```rust
pub struct StatementRange { pub start: usize, pub end: usize }  // byte offsets
pub fn split_statements(sql:&str) -> Vec<StatementRange>;        // sqlparser TokenizerWithSpan;
                                                                // semicolons terminate; comments/whitespace folded into
                                                                // preceding statement; trailing text w/o ';' = final range
pub fn statement_at(sql:&str, cursor:usize) -> Option<StatementRange>;

#[derive(Clone,Copy)] pub enum TokenKind { Keyword, String, Number, Comment, Identifier, Operator, Punctuation, Plain }
pub struct HighlightToken { pub start: usize, pub end: usize, pub kind: TokenKind }
pub fn highlight(sql:&str) -> Vec<HighlightToken>;              // TokenizerWithSpan; Keyword→is_keyword()
                                                                // Single/DoubleQuotedString→String; Multiline comment→Comment;
                                                                // Number→Number; Word w/ quoted→Identifier; fallback Plain
pub fn completions(prefix:&str, catalog:&[String]) -> Vec<String>; // case-insensitive prefix match over
                                                                // SQL_KEYWORDS const + catalog (table/column names); sorted, ≤50
```
Dialect: `GenericDialect` (MSSQL keywords mostly covered; add `&["TOP","IDENTITY","NVARCHAR","DATETIME2"]` via `is_keyword` extension table `EXTRA_KEYWORDS: &[&str]`).

**Steps:** rstest cases — single/multi statements, `;` inside string literal `'a;b'` does NOT split, `-- comment ;` does NOT split, `GO`-less input, cursor inside 2nd of 3, empty input → `[]`; highlight classifies `SELECT`, `'x'`, `--`, `1.5`. `cargo test -p datara-sql-editor`. Commit `feat(sql-editor): statement splitting and highlighting`.

### Task 4.2: Editor component + tabs + command system

**Files:** `crates/app/src/editor_ui.rs`, `ui/editor/query_editor.slint`, `ui/components/tabs.slint`, `ui/app.slint` (rework layout to §21 skeleton: toolbar / sidebar | editor / results / statusbar)

`query_editor.slint` — component `QueryEditor`:
- `TextEdit` (monospace `font-family: "JetBrains Mono", monospace` fallback `monospace`, `font-size` bound to config) in `Flickable`, `edited()` → `Bridge.editor-changed(text)`.
- Highlight overlay: Rust computes `Vec<HighlightToken>` → mapped to rows of `HighlightSpan{ text:string, color:color, x:length, y:length }` (precomputed monospace cell coords — chars*char_width, line*line_height). Delegate `Text` elements in same Flickable below the TextEdit (transparent text edit, `foreground` transparent? — implementer note: draw highlight layer UNDER a `TextEdit` with transparent text but visible cursor/selection: Slint `TextEdit` draws its own text; approach = keep TextEdit text visible with `foreground: Theme.fg` and apply coloring via per-token `Text` overlay behind is INVERTED — choose instead: TextEdit gets `foreground: transparent`, `Text` spans render the colored text at identical coordinates; verify alignment onscreen; fallback plan if misaligned: plain single-color text + line-number gutter + statement underline, still meets "syntax highlighting" via keyword-color Text layer tuned during Task 4.4 verification).
- Gutter: line-number column bound to `line-count` property; current line/col → `Bridge.cursor-position(line:int,col:int)` via `cursor-position-changed` → status bar.
- `in property<string> text` two-way; `property<int> cursor-offset`; autocompletion: `Bridge.editor-changed` → Rust `completions()` → `Bridge.completions` model → small popup `ListView` under cursor, `Tab`/`Enter` accepts → `Bridge.apply-completion(start:int,end:int,text:string)`; `Escape` closes.

Command system — `crates/domain/src/command.rs` already has `enum Command`; app side:
```rust
// app/commands.rs
pub fn command_for(ke: &slint::platform::KeyEvent /* or (Key, modifiers) */) -> Option<Command>; // Ctrl+Enter→ExecuteQuery …
pub struct Commands { tx: tokio::sync::mpsc::UnboundedSender<Command> }
```
Shortcuts handled in ONE `FocusScope`/`key-pressed` handler in `app.slint` → `Bridge.command(string)` → Rust maps string→Command→mpsc. Map per spec §20.

Tabs: `tabs.slint` — `in property<[TabItem]> tabs` {id:int,title:string}, `active:int`; `Bridge.new-tab/close-tab/switch-tab`; each tab = `{id, text, conn_id}` owned by `EditorState` in `app/editor_ui.rs` (`Vec<EditorTab>`, `switch` swaps text into/out of per-tab storage — slint has single editor instance; content stored per tab in Rust).

**Steps:** `command_for` unit tests (Ctrl+Enter→ExecuteQuery, Ctrl+Shift+P→OpenPalette, bare key→None). Build; smoke: type SQL, gutter shows line numbers, status bar shows `Ln X, Col Y`. Commit `feat(app): editor component, tabs, command dispatch`.

### Task 4.3: Execute path end-to-end + cancellation

**Files:** `crates/app/src/bridge.rs` (`AppEvent::{QueryStarted{tab}, QueryResult{tab, result}, QueryError{tab, message}, QueryCancelled{tab}}`), `ui/components/toolbar.slint`, `ui/grid/result_grid.slint` (minimal placeholder grid — replaced Phase 5)

`ExecuteQuery` command → resolve active tab's `conn_id` + database → text: selection if any (`editor.has-selection` property → `Bridge.selected-text` callback) else `statement_at(cursor)` else whole doc → `service.execute(...)` → `JoinHandle` stored in `running: HashMap<tab,QueryHandle>` → on completion `QueryResult` event → grid model + `HistoryRepo::record` (service already records; UI only renders).

Cancel: toolbar Stop button → `Bridge.cancel-query` → `running.remove(tab).abort()`.

**Steps:** unit test `resolve_sql(text, selection:(usize,usize)|None, cursor)` → correct slice for each mode. Live smoke vs container: `SELECT 1` in editor → result in placeholder grid; `SELECT WAITFOR DELAY '00:00:30'` → cancel → QueryCancelled. Commit `feat(app): async query execution and cancellation`.

### Task 4.4: Editor verification pass

Compare rendered highlights vs `highlight()` tokens on `SELECT id, 'x;y' -- c` — screenshot via Slint's `--save-screenshot`? Not available; verify by `niri msg screenshot`? Manual visual check acceptable: run app, type sample, confirm keyword colors + no drift at 40+ lines. Log verdict in task notes; if overlay drifts, ship single-color + gutter fallback and file follow-up issue. Commit `fix(editor): alignment` or note pass.

**Phase 4 gate:** type SQL → Ctrl+Enter → async results, selection vs statement modes correct, errors show server line/col in status bar, no UI freeze (spin test: run `WAITFOR` → editor still types).

---

# PHASE 5 — Data grid

### Task 5.1: RowCache + VirtualizedRows model

**Files:** `crates/data-grid/src/{lib,cache,model}.rs`

**Produces:**
```rust
pub struct RowCache { columns: Vec<QueryColumn>, rows: Vec<QueryRow>, total_hint: AtomicUsize, truncated: bool }
impl RowCache {
    pub fn from_result(r:QueryResult) -> Self;
    pub fn row_count(&self) -> usize;
    pub fn cell(&self, row:usize, col:usize) -> &Value;
    pub fn copy_cells(&self, sel:&CellSelection) -> String;  // TSV; selection types below
}
#[derive(Clone)] pub struct CellSelection { pub anchor:(usize,usize), pub head:(usize,usize) } // normalized bounds via .bounds()
pub struct VirtualizedRows { cache: Rc<RowCache> }         // slint::Model adapter
impl slint::Model for VirtualizedRows { type Data = ModelRc<SharedString>; row_count(); row_data(i) → ModelRc<VecModel<SharedString>> }
```
Row data materializes `SharedString` per cell lazily per visible row (ListView only asks for visible rows). `copy_cells`: tab-separated, NULL → empty, rows joined `\n`.

**Steps:** tests — `bounds()` normalizes reversed selection, `copy_cells` TSV shape + NULL→"", `row_data` strings match `Value::Display`. Commit `feat(data-grid): row cache and slint model`.

### Task 5.2: Result grid component

**Files:** `ui/grid/result_grid.slint`

`ResultGrid` component:
- `in property<[GridCol]> columns` {name:string,data-type:string,width:int}; `in property<ListView-model>` via `in property<[[string]]>`? — Slint: `in property<ModelRc<ModelRc<SharedString>>>` impossible in .slint syntax; use `in property<[length]> row-count` + `callback cell-text(row:int,col:int)->string`? — callback-per-cell is too chatty. Instead: `rows` exposed as `in property<[GridRow]>` where `GridRow{cells:[string]}` — Slint supports nested array structs `{ cells: [string] }`; Rust `ModelRc<GridRow>` where `GridRow` slint-struct has `cells: ModelRc<SharedString>` — from Rust: `GridRow{ cells: ModelRc::from(VecModel::from_slice(...)) }`. ListView over rows (virtualized), each row delegate `HorizontalLayout` of cell `Text` width bound per column via `columns[i].width`.
- Header row: `HorizontalLayout` headers + 4px drag handles → `Bridge.resize-column(idx:int, delta:length)` → updates `columns[i].width`.
- Outer `Flickable` for horizontal scroll; ListView inside sized to total columns width.
- Selection: click cell → `Bridge.grid-select(row,col)`; `Ctrl+C` → `Bridge.copy-selection` → `cache.copy_cells` → `slint::platform::set_clipboard_text` (verify API exists in 1.18; else `arboard`).
- NULL: cell text "NULL", `color: Theme.fg`, `opacity:0.45`, `font-italic:true` via `cell.is-null: bool` in GridRow cells — change GridRow.cells to `[GridCell]{text:string,is-null:bool}`.
- `in property<int> total-rows`, footer "N rows (truncated at 1000)" via `in property<bool> truncated`.

**Steps:** build; smoke — container query `SELECT TOP 5000` tall/wide table → scroll smooth, resize columns, copy 2×2 range → paste into editor shows TSV. Commit `feat(ui): virtualized result grid`.

### Task 5.3: Wire open-table preview + sorting

**Files:** `crates/app/src/bridge.rs`, `crates/data-grid/src/model.rs`

`open-table` → `preview_sql` → `service.execute` → grid. Sorting: header click → `Bridge.sort-column(idx)` → Rust sorts `RowCache.rows` by cell `Value` (numeric-aware: Int/Float numeric compare, else string; Nulls last; toggle asc/desc) → model reset. Sort is client-side only (already materialized ≤max_rows) — ponytail comment: server-side ORDER BY when lazy paging lands.

**Steps:** unit tests `sort_rows(col,dir)` numeric vs text vs null ordering. Smoke: sort by int column, then nvarchar. Commit `feat(grid): preview wiring and column sort`.

**Phase 5 gate:** 5000-row result scrolls without jank (observe), column resize/sort/copy/NULL all work, truncated footer shown.

---

# PHASE 6 — Query history

### Task 6.1: History UI + wiring

**Files:** `ui/pages/history.slint`, `crates/app/src/history_ui.rs`

`HistoryPanel`: search `LineEdit` → `Bridge.history-search(text)`; `ListView` entries {id,query(first line,80ch),started_at,duration,rows,success}; row buttons: Rerun→`Bridge.history-rerun(id)` (loads into editor + execute), Copy→`Bridge.history-copy(id)`, Delete→`Bridge.history-delete(id)`. Toggle: `Command::SearchHistory` / sidebar button.

`history_ui.rs`: `HistoryService { search(filter) → Vec<HistoryItem> }` calling `HistoryRepo`; `rerun(id)` → get query → event `LoadQuery{text}`.

**Steps:** repo already tested (Task 1.5); test `HistoryItem::from(entry)` formatting (duration "1.2s", timestamp RFC3339→local). Smoke: run 3 queries → open panel → search substring → rerun → grid updates. Commit `feat(app): query history panel`.

**Phase 6 gate:** queries recorded automatically, searchable, rerunnable, deletable; no secrets in history (enforced by type — `QueryHistoryEntry` has no credential field; assert in test already done Task 1.5).

---

# PHASE 7 — MCP server

### Task 7.1: rmcp stdio server skeleton

**Files:** `crates/mcp-server/src/{lib,server,tools}.rs`, `crates/app/src/mcp_main.rs`, `crates/app/src/main.rs` (`mcp-serve` subcommand)

```rust
// server.rs
#[derive(Clone)] pub struct DataraMcp { services: Arc<AppServices>, cfg: McpConfig, tool_router: ToolRouter<Self> }
#[tool_router] impl DataraMcp { /* tools in tools.rs via #[tool] methods */ }
#[tool_handler] impl ServerHandler for DataraMcp { fn get_info(&self)->ServerInfo { protocol_version, capabilities: tools, server_info{name:"datara",version}, instructions:"MSSQL inspection + queries. Tools: list_connections, list_databases, list_tables, describe_table, search_schema, execute_query" } }
pub async fn serve_stdio(services:Arc<AppServices>, cfg:McpConfig) -> anyhow::Result<()> {
    let server = DataraMcp::new(services, cfg);
    let service = server.serve(rmcp::transport::stdio()).await?;
    service.waiting().await?; Ok(())
}
```
`mcp_main.rs` — `pub async fn run()->Result<()>`: tracing→stderr (`with_writer(std::io::stderr)`, NEVER stdout — stdio transport is stdout), `Storage::open`+`SecretStore::connect`+`DatabaseService::new`+`serve_stdio`. Gate on `cfg.mcp.enabled` → print stderr "MCP disabled — set mcp.enabled=true in config" exit 0? — spec wants safe default: `enabled=false` → refuse with message; docs cover enabling.

`main.rs` `Cmd::McpServe` → `runtime.block_on(mcp_main::run())`.

**Steps:** build; `echo '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{...}}' | ./datara mcp-serve` handshake sanity (manual); commit `feat(mcp): stdio server skeleton`.

### Task 7.2: MCP tools

**Files:** `crates/mcp-server/src/tools.rs`

```rust
#[derive(Deserialize, JsonSchema)] pub struct ConnectionIdParam { pub connection_id: i64 }
#[derive(Deserialize, JsonSchema)] pub struct ListTablesParam { pub connection_id: i64, pub database: String, pub schema: Option<String> } // schema default "dbo"
#[derive(Deserialize, JsonSchema)] pub struct DescribeParam { pub connection_id: i64, pub database: String, pub schema: Option<String>, pub table: String }
#[derive(Deserialize, JsonSchema)] pub struct SearchParam { pub connection_id: i64, pub database: String, pub pattern: String }
#[derive(Deserialize, JsonSchema)] pub struct ExecuteParam { pub connection_id: i64, pub database: String, pub query: String }
```

Tools (all `-> Result<CallToolResult, McpError>`, `CallToolResult::success(vec![Content::json(...)])`):
- `list_connections()` → `[{id,name,host,port,database,username}]` — mapped from profiles; NEVER `secret_reference`/password fields.
- `list_databases(p)` → `[{name}]`.
- `list_tables(p)` → `[{schema,name,kind}]` (kind "table"/"view").
- `describe_table(p)` → `{schema,name,columns:[{name,data_type,nullable,is_primary_key,ordinal}],indexes:[{name,columns,is_unique}]}`.
- `search_schema(p)`: `list_tables` all schemas + `describe_table` each (≤200 tables — ponytail: cap, server-side WHERE upgrade later) → case-insensitive `contains` on schema/table/column names → `[{schema,table,matched_column?}]`.
- `execute_query(p)` → classify via `sqlparser` → `statement_type:"read"|"write"`; if write AND `!cfg.mcp.execute.allow_writes` → `McpError::invalid_params("write queries disabled")`; else `service.execute(id, Some(db), q, cfg.mcp.max_result_rows)` → `.await` handle → JSON `{columns,rows,row_count,truncated,statement_type}`; errors → `McpError::internal_error(sanitized)` (never include credentials — DomainError Display already safe).

`AppServices` needs a facade usable from both UI + MCP: add `services.rs` methods `list_connection_summaries()`, `list_databases(id)`, `list_tables(...)`, `describe_table(...)`, `execute(id,db,query,max)->QueryResult` — thin delegates to `DatabaseService`+`ConnectionRepo` (already exists; MCP gets `Arc<AppServices>`).

**Steps:** unit tests — param schema validates (missing connection_id → error), `list_connections` output has no secret fields (serialize→assert JSON keys), write-query gate honors `allow_writes=false`, truncation flag when result capped (fake `DatabaseService`? service is concrete — test via storage+in-memory sqlx + stub `DatabaseSession` impl injected — refactor `DatabaseService` to hold `driver: Arc<dyn DriverFactory>`? Keep simple: `DatabaseService` gains `#[cfg(test)]` constructor with a `MockSession` implementing `DatabaseSession` returning canned rows — write tests against that). Live test via container optional in Task 7.3. Commit `feat(mcp): tools`.

### Task 7.3: MCP end-to-end + docs

**Files:** `docs/mcp/tools.md`, `docs/mcp/security.md`, `README.md` (MCP setup section)

E2E test in `crates/mcp-server/tests/stdio.rs`: spawn `datara mcp-serve` with env `DATARA_MCP_TEST=1`? Simpler: spawn rmcp client in-process: `().serve(child_process tokio::process::Command("datara-mcp-serve"))`? rmcp has `TokioChildProcess` transport (`transport-child-process` feature) — spawn `cargo run -p datara -- mcp-serve`; client `initialize`, `list_tools`, call `list_connections` (empty db → `[]`), insert profile via `Storage` in same temp HOME → `list_connections` returns it. Env override: `DATARA_DATA_DIR`/`XDG_DATA_HOME` to tempdir for isolation — support `DATARA_DATA_DIR` env in `AppPaths::new()` (add Task 7.3 patch to config).

`docs/mcp/tools.md`: table per tool (params/returns/limits). `docs/mcp/security.md`: threat model + what never crosses the boundary + enable instructions + Claude Code snippet:
```json
{ "mcpServers": { "datara": { "command": "datara", "args": ["mcp-serve"] } } }
```

**Steps:** `cargo test -p datara-mcp-server`; docs; commit `feat(mcp): end-to-end tests and docs`.

**Phase 7 gate:** `list_connections`/`list_databases`/`list_tables`/`describe_table`/`search_schema`/`execute_query` all callable over stdio; truncation honored; write-gate enforced; zero secret leakage (JSON-key assertion test).

---

# Additional tasks (inserted during self-review)

### Task 4.5: Command palette + tree filter

**Files:** `ui/components/palette.slint`, `crates/app/src/commands.rs`

`Palette` overlay: `LineEdit` + `ListView` of `CommandItem{label:string,command:string}` — static list of all `Command` variants + dynamic "Open connection: <name>" entries. Fuzzy filter: case-insensitive subsequence match (10-line `fn fuzzy(needle,hay)->Option<score>` — ponytail: no ranking beyond match order). `Bridge.command(string)` dispatch reuses existing path. `Ctrl+K` → tree filter `LineEdit` over sidebar filtering visible `TreeNode`s by label substring (client-side over already-loaded nodes).

**Steps:** unit tests `fuzzy` (subsequence match/no-match/case). Build; smoke `Ctrl+P`→type "exe"→Enter executes query. Commit `feat(app): command palette and schema filter`.

### Task 6.2: Saved queries (Ctrl+S)

**Files:** `crates/storage/src/saved.rs` + `ui/dialogs/save_query.slint` + history panel tab "Saved"

Migration `0002_saved.sql` already covered by `saved_queries` table from `0001_init.sql` — no new migration. `SavedRepo`: `save(name,query)->i64`, `list()->Vec<SavedQuery>`, `delete(id)`, `get(id)->SavedQuery`. `Ctrl+S`/`SaveQuery` → dialog (name field) → `SavedRepo::save`. History panel gains a "Saved" tab listing entries → Open (load into editor tab), Delete.

**Steps:** storage tests `save/list/delete` round-trip. Smoke: Ctrl+S → name → appears in Saved tab → reopen. Commit `feat(storage): saved queries`.

# PHASE 8 — Packaging

### Task 8.1: Desktop integration assets

**Files:** `assets/icons/datara.svg`, `packaging/share/io.github.ntxinh.Datara.desktop`, `packaging/share/io.github.ntxinh.Datara.metainfo.xml`

Icon: simple 512px SVG — rounded square `Theme.accent` (#89b4fa) background, white table-grid glyph (3×3 cells). `.desktop`:
```ini
[Desktop Entry]
Type=Application
Name=Datara
Comment=Native MSSQL database client
Exec=datara
Icon=io.github.ntxinh.Datara
Terminal=false
Categories=Development;Database;
StartupWMClass=datara
Keywords=sql;mssql;database;query;
```
`StartupWMClass=datara` — Wayland app_id is set by winit from the binary name; the implementer must verify with `WAYLAND_DEBUG=1`/`niri msg windows` that the surface reports `app_id` containing "datara"; if it reports something else, set `StartupWMClass` to the observed value and document in `docs/development/debugging.md`.

`metainfo.xml`: `id=io.github.ntxinh.Datara`, name, summary, `<launchable>io.github.ntxinh.Datara.desktop</launchable>`, `developer_name`, `project_license=GPL-3.0-only`, `releases`, `content_rating type="oars-1.1"`.

**Steps:** `desktop-file-validate` + `appstreamcli validate` (install `desktop-file-utils`/`appstream` — both packaged in Fedora; if `appstreamcli` absent, `xmllint --noout` the metainfo). Commit `feat(packaging): desktop assets`.

### Task 8.2: RPM spec + build

**Files:** `packaging/rpm/datara.spec`, `Makefile` (`rpm` target), `.github/workflows/release.yml` (`rpm` job)

```spec
Name:           datara
Version:        0.1.0
Release:        1%{?dist}
Summary:        Native MSSQL database client
License:        GPL-3.0-only
URL:            https://github.com/ntxinh/Datara
Source0:        %{name}-%{version}.tar.gz
BuildRequires:  rust >= 1.85, cargo, desktop-file-utils, libappstream-glib
BuildRequires:  pkgconfig(fontconfig), pkgconfig(freetype2), pkgconfig(xkbcommon), pkgconfig(wayland-client)
Requires:       hicolor-icon-theme
%description
Datara is a native Rust + Slint MSSQL database client for Linux/Wayland.
%prep
%autosetup
%build
cargo build --release --locked
%install
install -Dm755 target/release/datara %{buildroot}%{_bindir}/datara
install -Dm644 packaging/share/io.github.ntxinh.Datara.desktop %{buildroot}%{_datadir}/applications/io.github.ntxinh.Datara.desktop
install -Dm644 packaging/share/io.github.ntxinh.Datara.metainfo.xml %{buildroot}%{_datadir}/metainfo/io.github.ntxinh.Datara.metainfo.xml
install -Dm644 assets/icons/datara.svg %{buildroot}%{_datadir}/icons/hicolor/scalable/apps/io.github.ntxinh.Datara.svg
%files
%license LICENSE
%{_bindir}/datara
%{_datadir}/applications/io.github.ntxinh.Datara.desktop
%{_datadir}/metainfo/io.github.ntxinh.Datara.metainfo.xml
%{_datadir}/icons/hicolor/scalable/apps/io.github.ntxinh.Datara.svg
%changelog
* Sun Sep 27 2026 Datara Developers - 0.1.0-1
- Initial release
```
`make rpm`: `rpmbuild -ba` with `_topdir` under `packaging/rpm/_build` + `spectool -g` for the source tarball produced by `git archive --format=tar.gz --prefix=datara-0.1.0/ HEAD`. CI `rpm` job in release.yml: `container: fedora:44`, `dnf install -y rpm-build rust cargo ...`, `make rpm`, upload `*.rpm` artifacts.

**Steps:** `rpmlint` the spec if available (ignore missing); `make rpm` builds `.rpm` in container or mock — CI must produce it; local run optional. Commit `feat(packaging): RPM spec and CI job`.

### Task 8.3: Flatpak manifest + build

**Files:** `packaging/flatpak/io.github.ntxinh.Datara.yml`, `packaging/flatpak/cargo-sources.json`, `Makefile` (`flatpak` target)

```yaml
app-id: io.github.ntxinh.Datara
runtime: org.freedesktop.Platform
runtime-version: '24.08'
sdk: org.freedesktop.Sdk
command: datara
finish-args:
  - --socket=wayland
  - --socket=fallback-x11
  - --share=network            # required: connects to SQL Server
  - --socket=session-bus       # required: Secret Service D-Bus
  - --filesystem=xdg-documents  # NOT requested unless a real need appears — remove
modules:
  - name: datara
    buildsystem: simple
    build-commands:
      - cargo build --release --offline --locked
      - install -Dm755 target/release/datara /app/bin/datara
      - install -Dm644 packaging/share/io.github.ntxinh.Datara.desktop /app/share/applications/io.github.ntxinh.Datara.desktop
      - install -Dm644 packaging/share/io.github.ntxinh.Datara.metainfo.xml /app/share/metainfo/io.github.ntxinh.Datara.metainfo.xml
      - install -Dm644 assets/icons/datara.svg /app/share/icons/hicolor/scalable/apps/io.github.ntxinh.Datara.svg
    sources:
      - type: dir
        path: ../..
      - cargo-sources.json
```
`cargo-sources.json`: generated by `flatpak-cargo-generator.py Cargo.lock -o packaging/flatpak/cargo-sources.json` (fetch script at task time: `curl -L https://raw.githubusercontent.com/flatpak/flatpak-builder-tools/master/cargo/flatpak-cargo-generator.py`); commit the generated file. Remove the `--filesystem` line shown above (left in yaml sketch to make the instruction explicit: network+wayland+session-bus ONLY).
`make flatpak`: `flatpak-builder --force-clean --install-deps-from=flathub build packaging/flatpak/io.github.ntxinh.Datara.yml && flatpak build-export repo build && flatpak build-bundle repo datara.flatpak io.github.ntxinh.Datara`.
Host `$HOME` filesystem: NOT granted — XDG dirs map into `~/.var/app/io.github.ntxinh.Datara/{data,config,state}` automatically; `dirs` crate resolves XDG vars which flatpak sets correctly. Document this path difference in `docs/packaging/flatpak.md`.

**Steps:** `make flatpak` produces `datara.flatpak`; install + run smoke optional (host env present). Update `docs/packaging/{rpm,flatpak}.md` + `release.yml` flatpak job (`container: bilelmoussaoui/flatpak-github-actions` or `flatpak-builder` action — use `flatpak/flatpak-github-actions@v8` if reachable; else job documented as manual). Commit `feat(packaging): Flatpak manifest`.

# PHASE 9 — Hardening

### Task 9.1: Quality gates + config audit + workspace state

**Files:** `crates/app/src/main.rs` (config consumption), `crates/config/src/settings.rs` (`WorkspaceState`), `docs/ui/components.md`, `rust-toolchain.toml`

- `WorkspaceState{sidebar_width:u32=240, editor_split:f32=0.5, open_tabs:Vec<String>=[]}` in `settings.toml` `[workspace]` table, saved on window close (`slint::Window::on_close_requested` or hide event), loaded at startup → binds to sidebar width + split position.
- Config audit: every `AppConfig` field must be consumed somewhere — `editor.font_size`→TextEdit, `editor.tab_size`→insert spaces, `query.default_limit`→preview+`max_rows` default, `query.timeout_seconds`→`tokio::time::timeout(Duration)` wrapping `service.execute` handle → `DomainError::Cancelled` on timeout, `appearance.theme`→`Theme.dark`, `mcp.*`→Task 7.x. Add test listing all keys.
- Full gate: `make fmt-check lint test build` + `cargo audit` + `cargo deny check` clean; prune unused workspace deps (`cargo machete` if available else manual `cargo udeps` skip — document what remains).
- `rust-toolchain.toml`: `[toolchain] channel="1.98.1"` — keeps rust-analyzer/cargo aligned with mise pin. Add `rust-toolchain.toml` note to `docs/development/setup.md`.
- `docs/ui/components.md`: component inventory (Window→Layout→Sidebar/Tree/Editor/Grid/Dialogs) + Bridge callback list.

**Steps:** all green; commits `chore: quality gates`, `feat(config): workspace state`.

### Task 9.2: Benchmarks + performance notes

**Files:** `crates/sql-editor/benches/statements.rs`, `crates/data-grid/benches/cache.rs`, `crates/domain/benches/value.rs` (display formatting), `docs/development/performance.md`

Criterion benches: `split_statements` 100-statement 10KB doc; `highlight` same doc; `RowCache` build 10k×20 + `row_data` ×64 viewport; `Value::Display` hot loop. Startup: `hyperfine` absent → `time ./datara` 3× cold+note (headless skip — record manual observation). Document numbers in `performance.md` (table: metric, result, env). No regression gates — baselines only (ponytail: no bencher CI until drift matters).

**Steps:** `cargo bench` runs; doc written; commit `perf: benchmarks and baselines`.

### Task 9.3: Security review pass + doc sync + rust-analyzer pin

**Files:** `docs/security/{secrets,tls}.md`, `docs/mcp/security.md`, `AGENTS.md`, `README.md` (status), `DESIGN.md` (sync to final), `SECURITY.md` (root, reporting policy)

Review checklist (verify + document verdicts):
- `rg -n 'password|secret' --type rust` → no `Debug`-derive on `Credentials` (manual `impl Debug` redacts; assert with test `format!("{:?}", creds)` contains no secret), no `tracing` field carries `password`/`secret`/`token` (grep `instrument(skip`), `DomainError` messages built without credential values.
- SQLite file mode `0600` (set after open via `fs::set_permissions`) + test.
- `secret_reference` in profile ≠ secret value (unit test asserts `serde_json` of `ConnectionProfile` contains the ref string, not password — trivially true, but locks the invariant).
- TLS matrix documented in `docs/security/tls.md`: Disabled/Preferred/Required × trust_cert — what each means with Tiberius; cert-validation error UX.
- MCP boundary re-verified: `list_connections` JSON keys, `execute_query` gates.
- README status updated to MVP-complete; DESIGN.md matches shipped architecture; AGENTS.md commands verified verbatim against Makefile.
- `SECURITY.md`: vuln reporting (GH private advisory link), supported versions, credential-handling summary.
- Wayland app_id verification result from Task 8.1 recorded in `docs/development/debugging.md` (how to check via `niri msg`/`WAYLAND_DEBUG`).

**Steps:** grep passes, docs written, `make docs-check` green, final `cargo build --release` + run. Commit `docs: security review and final sync`, `docs(security): disclosure policy`.

**Phase 9 gate:** release binary runs full MVP flow end-to-end on Wayland; CI green on all 4 workflows; `docs/README.md` index resolves; spec §34 Definition-of-Done checklist walked item by item and logged in the task report.

---

# Deviations and simplify notes

- Top-level `tests/` carries only `README.md`; real integration tests live in `crates/driver-mssql/tests/` and `crates/mcp-server/tests/` because `cargo test --workspace` only discovers member-package targets. Spec layout otherwise identical.
- `saved_queries` table ships in `0001_init.sql` (used by Task 6.2 — no second migration needed).
- `Task 4.2` highlight overlay has an explicit fallback (single-color + gutter) if per-token `Text` alignment drifts — implementer picks after Task 4.4 visual check.
- MCP `mcp.enabled` defaults `false` (safe default per spec §17); README + `docs/mcp/overview.md` cover enabling.
- Connection `cancel()` is abort-via-drop: the service aborts the execute `JoinHandle`, dropping the TCP stream; TDS attention-message cancellation is a later upgrade (comment in `service.rs`).
- Ctrl+K maps to schema-tree filter (spec §20's generic "Search"); Ctrl+P palette.
