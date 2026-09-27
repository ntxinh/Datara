# Datara — Design Spec

**Date:** 2026-09-27
**Status:** Approved
**Source:** Build specification supplied by the user (49 sections); this document records the locked decisions and distills the requirements the implementation plan must satisfy. The full build spec is the authoritative requirements text and is preserved in the project history.

## Locked Decisions

| Decision | Value |
|---|---|
| Product name | **Datara** |
| Application ID | `io.github.ntxinh.Datara` |
| License | **GPL-3.0** (required by Slint's free license tier) |
| Delivery scope | **All 9 phases** — full MVP per build spec §34 |
| Remote | `github.com/ntxinh/Datara` |
| Container runtime for tests | **Podman** (Docker absent) |

## Product

Native Linux MSSQL database client (TablePlus-style), Rust + Slint, Wayland-native. Target environment: Fedora 44, Niri, Fish shell — but only standard Wayland/desktop integration, no compositor coupling.

**MVP:** MSSQL only. PostgreSQL/SQLite drivers are designed-for but not implemented. No AI features, SSH tunnels, telemetry, accounts, SaaS.

## Stack

- Rust (pinned via `mise.toml`), Cargo workspace, Tokio
- Slint (GPL-3.0) declarative UI, winit/Wayland
- Tiberius (MSSQL), rustls TLS
- SQLite (bundled rusqlite) for local state — never for secrets
- Secret Service API (GNOME Keyring; visible in Seahorse)
- `rmcp` stdio MCP server sharing the app's database services
- sqlparser-rs for statement detection; tree-sitter only if practical
- tracing/tracing-subscriber, thiserror + anyhow, clap
- rustfmt, clippy, cargo-audit, cargo-deny; rstest, testcontainers, criterion
- GitHub Actions CI; RPM + Flatpak packaging

## Workspace Layout

```
crates/: app, domain, database, driver-mssql, sql-editor, data-grid,
         secrets, mcp-server, storage, config
ui/: app.slint, components/, pages/, dialogs/, editor/, grid/, theme/
packaging/: rpm/, flatpak/
docs/, tests/, .github/workflows/
```

Dependency direction: `UI → app → domain → infrastructure → drivers`. Domain never depends on Slint; drivers never depend on UI; MCP reuses application services — no second database layer.

## Phase Gates

1. **Foundation** — workspace, tooling files (`mise.toml`, `.editorconfig`, `Makefile`, `AGENTS.md`, `DESIGN.md`, `README.md`, `docs/` tree + index, `.omp/lsp.json`), CI workflows, minimal Slint window launching on Wayland with app ID, tracing, typed errors.
2. **Database** — domain types, `DatabaseDriver`/`DatabaseSession` traits, Tiberius MSSQL driver, connection form, Secret Service, TLS modes, `SELECT 1` proven against a real containerized SQL Server.
3. **Schema explorer** — connection tree: databases → schemas → tables → columns, refresh.
4. **SQL editor** — highlighting, statement detection, Ctrl+Enter execute, execute selection/document, cancellation, error reporting. Never blocks the UI thread.
5. **Data grid** — viewport virtualization (no per-cell components), scrolling, selection, column resize/sort, copy cell/row/selection, NULL display, type formatting; responsive at tens of thousands of rows.
6. **Query history** — SQLite persistence, search, rerun, copy, delete.
7. **MCP** — stdio server (`rmcp`); tools: `list_connections`, `list_databases`, `list_tables`, `describe_table`, `search_schema`, `execute_query`. Configurable max rows (default 1000), truncation reporting, read/write distinction, never returns secrets. Off-by-default posture for network; stdio only.
8. **Packaging** — RPM, Flatpak (minimal permissions + network), desktop entry, icons, AppStream metadata.
9. **Hardening** — clippy clean, cargo-audit/deny, MCP security review, benchmarks (startup, schema load, result processing, grid), docs final pass.

Each phase ends: fmt + clippy + tests green, `cargo build` succeeds, docs updated — repo always buildable.

## Non-negotiables

- Passwords only in Secret Service; never in SQLite/TOML/logs/errors. Store `SecretReference` in `ConnectionProfile`, not the password.
- TLS never silently downgraded; cert errors surface clearly; `trust_server_certificate` is a deliberate user choice.
- Table preview = `SELECT TOP N` with configurable limit (default 1000); never unbounded `SELECT *`.
- All DB I/O on Tokio; UI thread never blocks.
- MCP: explicit `connection_id` + `database` + `query`; no arbitrary hosts; no credential exposure.
- XDG paths (`~/.local/share`, `~/.config`, `~/.local/state`) — no hard-coded home paths.
- Command/action system for shortcuts (Ctrl+Enter execute, Ctrl+S save, Ctrl+N/W/Tab, Ctrl+F, Ctrl+K, Ctrl+P) — not hard-coded in components.

## Definition of Done (MVP)

Per build spec §34: install → launch on Wayland/Niri → create MSSQL connection → password via Secret Service → connect → browse databases/schemas/tables/columns → safe table preview → write & execute SQL async → virtualized results + copy → history + rerun → TLS config → start MCP stdio → list/inspect/query via MCP client → RPM/Flatpak install → CI green.

## Documentation

`README.md`, `DESIGN.md`, `AGENTS.md`, `docs/` (architecture, database, mcp, security, development, packaging, decisions/ADRs) with `docs/README.md` index. Docs updated in the same change as behavior — never deferred.
