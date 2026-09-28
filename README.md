# Datara

A native Linux database client for **Microsoft SQL Server**, written in Rust
with a Slint UI. TablePlus-style: fast, keyboard-first, no Electron.

- **Platform:** Linux, Wayland-native. Developed on Fedora 44 with the niri
  compositor; only standard Wayland/desktop integration is used, so it runs on
  any Wayland session (and falls back to X11 via winit).
- **Database support:** MSSQL only for the MVP. PostgreSQL and SQLite drivers
  are designed-for but deliberately not implemented yet.
- **License:** GPL-3.0 (required by Slint's free license tier).

## Features

| Feature | Status |
|---|---|
| Workspace, domain types, config, storage, minimal window | ✅ done |
| Wayland launch (app_id `datara`; packaging ID `io.github.ntxinh.Datara`) | ✅ done |
| MSSQL connection over TDS 7.3 (Tiberius) | ✅ done |
| Passwords via Secret Service (GNOME Keyring) | ✅ done |
| TLS modes with explicit certificate policy | ✅ done |
| Schema explorer (databases → schemas → tables → columns) | ✅ done |
| SQL editor: highlighting, statement detection, Ctrl+Enter | ✅ done |
| Virtualized data grid with copy/export | ✅ done |
| Query history in local SQLite | ✅ done |
| MCP server (stdio) for agentic clients | ✅ done |
| RPM and Flatpak packaging | ✅ done |

## Install

From source (below) or via packaging artifacts — see
`docs/packaging/rpm.md` and `docs/packaging/flatpak.md`. A desktop entry
plus AppStream metadata install under the standard `/usr/share` paths.

## Development

Requires [mise](https://mise.jdx.dev) and a C toolchain plus fontconfig
headers (`dnf install fontconfig-devel`).

```sh
mise install      # pinned Rust toolchain (mise.toml)
make build        # cargo build --workspace
make run          # launches the GUI
make test         # unit + integration tests
```

See `AGENTS.md` for the full command list and `docs/development/setup.md` for
details.

## MCP server

`datara mcp-serve` exposes connections and query tools over stdio to agentic
clients (six tools: list_connections, list_databases, list_tables,
describe_table, search_schema, execute_query). Opt-in — set `mcp.enabled =
true` in `config.toml`, then point the client at `datara mcp-serve`.
Setup: `docs/integrations/mcp.md`.

## Security

- Passwords are stored **only** in the system Secret Service; the local
  SQLite database and config TOML keep a `SecretReference`, never the secret.
- TLS is never silently downgraded; `trust_server_certificate` is an explicit
  per-connection choice.
- Table previews run `SELECT TOP N` (default 1000) — never unbounded scans.

## Project status

**MVP complete.** All nine phases shipped: MSSQL connections over TLS with
Secret Service credentials, schema explorer, SQL editor with highlighting
and async execution, virtualized results grid, query history and saved
queries, a stdio MCP server, and RPM/Flatpak packaging. See `SECURITY.md`
for the disclosure policy.

## Roadmap

1. Foundation — workspace, tooling, docs, minimal window ✅
2. Database — MSSQL driver, Secret Service, TLS, connection UI ✅
3. Schema explorer ✅
4. SQL editor — highlighting, execution, cancellation ✅
5. Data grid — virtualization, copy, formatting ✅
6. Query history ✅
7. MCP server (stdio) ✅
8. Packaging — RPM, Flatpak, desktop integration ✅
9. Hardening — lint/audit gates, benchmarks, security review ✅

Details: `DESIGN.md`, `docs/`, `docs/superpowers/specs/`.
