# Build Specification: Native Linux MSSQL Database Client (original)

> Authoritative requirements document supplied by the user on 2026-09-27.
> Archived verbatim. `2026-09-27-datara-design.md` records the locked
> decisions on top of this spec.

## 1. Role

Senior Rust desktop application engineer. Native Linux database client inspired by TablePro/TablePlus, optimized for Fedora Workstation 44, Wayland, Niri compositor, DankMaterialShell, Fish shell. Native Rust + Slint. NO Electron, Tauri, React, WebView, GTK, Qt, Java, Python, Node.js for the runtime. PostgreSQL and SQLite drivers addable later without major architectural changes.

## 2. Product Goal

Features: connection management, SQL Server support, SQL editor, syntax highlighting, basic autocomplete, schema explorer, table browsing, virtualized data grid, query execution, query results, query history, local storage, Secret Service/GNOME Keyring, TLS, MCP server, native Wayland, Fedora packaging, GitHub Actions CI/CD.

MVP excludes: AI/LLM, AI SQL, chat, PostgreSQL, remote-SQLite, MySQL, Redis, SSH tunneling, cloud sync, plugin marketplace, telemetry, accounts, SaaS. SQLite only for internal metadata.

## 3. Technology Stack

Rust stable, Cargo, Tokio, Slint, Winit, Wayland. Slint UI, custom virtualized grid, custom SQL editor. Tiberius for MSSQL. sqlparser-rs; tree-sitter if practical. SQLite local storage (never passwords). Secret Service (GNOME Keyring → Seahorse inspectable). rustls. `rmcp` MCP server. clap if CLI needed. tracing/tracing-subscriber. thiserror + anyhow. cargo test, rstest, testcontainers, criterion. rustfmt, clippy, cargo-audit, cargo-deny. RPM + Flatpak. GitHub Actions.

## 4. Architecture

Cargo workspace; crates: app, domain, database, driver-mssql, sql-editor, data-grid, secrets, mcp-server, storage, config. ui/: app.slint, components, pages, dialogs, editor, grid, theme. assets/, tests/, packaging/{rpm,flatpak}, docs/, .github/workflows.

Dependency direction: UI → Application → Domain → Infrastructure → drivers. Domain: no Slint. Driver: no UI. MCP reuses application services.

## 5. Domain Model

Types: ConnectionProfile, DatabaseType, DatabaseConnection, Query, QueryResult, QueryColumn, QueryRow, DatabaseSchema, DatabaseInfo, TableInfo, ColumnInfo, QueryHistoryEntry, SavedQuery, Workspace, SecretReference. ConnectionProfile holds SecretReference, never password.

## 6. Database Abstraction

`DatabaseDriver` trait: connect / list_databases / get_schema / execute → `DatabaseSession`. Minimal, sufficient for MSSQL now + PostgreSQL/SQLite later.

## 7. MSSQL

Tiberius. host, port (default 1433, not hard-coded), database, username, password, SQL auth, TLS config, connect timeout, query timeout where supported.

## 8. TLS

Encryption: Disabled / Preferred / Required. Trust Server Certificate: bool. Never silently disable verification; clear cert errors; never recommend insecure config automatically.

## 9. Secret Service

`save_secret/load_secret/delete_secret(connection_id)`. SQLite stores reference (`mssql/<id>/password`); Secret Service stores actual value under app service/account. Seahorse-visible.

## 10. SQL Editor

Multi-line, line numbers, highlighting, selection, copy/paste, undo/redo, line/col display, basic autocomplete, execute statement/selection/document, Ctrl+Enter, error location. Never freezes.

## 11. Query Execution

Async: UI → command → Tokio → driver → SQL Server. Streaming/batched results; never bulk-load millions of rows.

## 12. Data Grid

Virtualized viewport (NOT per-cell components). Headers, h/v scroll, row+cell selection, column resize/sort, copy cell/row/selection, NULL rendering, type formatting, tens-of-thousands-rows responsive.

## 13. Schema Explorer

Left tree: Connections → databases → Tables/Views/Procs/Functions; tables expand to Columns/PK/FK/Indexes. MVP: tables + columns prioritized.

## 14. Table Browser

Double-click → `SELECT TOP 1000 * FROM dbo.X` (configurable limit); never unbounded.

## 15. Query History

SQLite: id, connection_id, database, query, started_at, duration_ms, row_count, success, error_message. Recent/search/rerun/copy/delete. No credentials.

## 16. MCP Server

stdio via `rmcp`; reuses app DB services. Tools: list_connections, list_databases, list_tables, describe_table, search_schema, execute_query (explicit connection_id+database+query).

## 17. MCP Security

stdio default, no network endpoint. Never: return passwords/secrets/keys, connect arbitrary hosts, bypass config/auth, disable TLS verification, execute on unspecified connection. Read vs write distinction; configurable destructive-SQL policy.

## 18. MCP Results

Structured JSON `{columns:[{name,type}], rows:[{...}], row_count}`. Max rows (default 1000); report truncation.

## 19. UI Design

Standard Wayland desktop app, `io.github.ntxinh.Datara` app ID. Resize, maximize, dialogs, keyboard nav, dark + light mode. No Niri/DMS coupling.

## 20. Keyboard-first UX

Ctrl+Enter execute, Ctrl+S save, Ctrl+K search, Ctrl+P/Ctrl+Shift+P palette, Ctrl+F find, Ctrl+Shift+F history, Ctrl+N new, Ctrl+W close, Ctrl+Tab next. Command/action system — not hard-coded in components.

## 21. UI Layout

Toolbar / sidebar (connections+schema) | editor / results grid / status bar. Resizable sidebar, editor/result split, columns.

## 22. Local DB

`~/.local/share/<app-id>/app.db`, `~/.config/<app-id>/`, `~/.local/state/<app-id>/`. XDG only.

## 23. Configuration

TOML: editor font/tab, query default_limit=1000/timeout=30, theme=system, mcp enabled+max_result_rows=1000. No secrets.

## 24. Performance

Responsive during connect/schema/query/large results/history search. No blocking I/O on UI thread. Benchmark startup, schema load, result processing, grid render, large sets.

## 25. Testing

Unit (parsing, config, statement detection, history, domain, MCP validation). Integration via testcontainers (connect/auth/list/describe/CRUD/transaction/TLS). MCP tool tests incl. errors, truncation, secret isolation, no password leakage.

## 26. Security

Never log/persist passwords (SQLite/TOML/errors); no secrets via MCP; no silent TLS downgrade; no arbitrary MCP hosts. Least privilege; all access via configured profiles.

## 27. Error Handling

User-readable errors ("Connection failed: could not connect to host:port — TLS certificate validation failed"), no raw debug dumps; diagnostics in logs.

## 28. GitHub Actions

ci.yml, security.yml, build.yml, release.yml. fmt --check, clippy -D warnings, test --workspace, audit, deny. x86_64 Linux; ARM64-ready.

## 29. Fedora Packaging

RPM + Flatpak; minimal permissions + network (DB access); no broad filesystem.

## 30. Documentation

README.md, ARCHITECTURE→DESIGN.md, DEVELOPMENT.md→docs/development, SECURITY.md→docs/security, MCP.md→docs/mcp, PACKAGING.md→docs/packaging, CONTRIBUTING.md. MCP docs cover stdio + Claude Code.

## 31. Phases

1 foundation → 2 database+secrets+TLS+SELECT 1 → 3 schema explorer → 4 editor → 5 grid → 6 history → 7 MCP → 8 packaging → 9 hardening. Per phase: fmt, clippy, tests, build, docs; repo always buildable.

## 32–35. Rules & DoD

Simple readable Rust; no speculative abstractions/deps; async only for real I/O; UI state ≠ domain state; no business logic in Slint; no DB in UI; no duplicated DB logic for MCP; no future drivers now. Full MVP DoD: install → Wayland launch → MSSQL connect → Secret Service password → browse → preview → async SQL → grid+copy → history+rerun → TLS → MCP stdio list/inspect/query → RPM/Flatpak → CI green.

## 36. mise

`mise.toml` pins Rust; docs use `mise install` / `mise exec -- cargo ...`.

## 37–48. Docs, Makefile, CI, LSP, .editorconfig

README, DESIGN.md, AGENTS.md, docs/ tree + docs/README.md index; doc-sync rule (code change → doc impact → same commit). .editorconfig (utf-8/lf/spaces; rs/slint/toml indent 4; md keeps trailing space). Makefile targets: setup/build/run/test/test-integration/lint/fmt/fmt-check/check/audit/deny/clean/docs-check. GH Actions use mise for toolchain. `.omp/lsp.json` → rust-analyzer.
