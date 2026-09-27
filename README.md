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
| MSSQL connection over TDS 7.3 (Tiberius) | 🚧 planned (phase 2) |
| Passwords via Secret Service (GNOME Keyring) | 🚧 planned (phase 2) |
| TLS modes with explicit certificate policy | 🚧 planned (phase 2) |
| Schema explorer (databases → schemas → tables → columns) | 🚧 planned (phase 3) |
| SQL editor: highlighting, statement detection, Ctrl+Enter | 🚧 planned (phase 4) |
| Virtualized data grid with copy/export | 🚧 planned (phase 5) |
| Query history in local SQLite | 🚧 planned (phase 6) |
| MCP server (stdio) for agentic clients | 🚧 planned (phase 7) |
| RPM and Flatpak packaging | 🚧 planned (phase 8) |

## Install

Packaging is pending (phase 8). For now, run from source — see below.

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

`datara mcp-serve` will expose connections and query tools over stdio to
agentic clients. Status: **planned** (phase 7) — the subcommand exists but
exits with "not yet implemented".

## Security

- Passwords are stored **only** in the system Secret Service; the local
  SQLite database and config TOML keep a `SecretReference`, never the secret.
- TLS is never silently downgraded; `trust_server_certificate` is an explicit
  per-connection choice.
- Table previews run `SELECT TOP N` (default 1000) — never unbounded scans.

## Project status

**Early development.** Phase 1 (foundation) is complete: the workspace,
domain types, XDG config, SQLite storage, driver traits, and a minimal Slint
window exist and build. Nothing beyond that works yet.

## Roadmap

1. Foundation — workspace, tooling, docs, minimal window ✅
2. Database — MSSQL driver, Secret Service, TLS, connection UI
3. Schema explorer
4. SQL editor — highlighting, execution, cancellation
5. Data grid — virtualization, copy, formatting
6. Query history
7. MCP server (stdio)
8. Packaging — RPM, Flatpak, desktop integration
9. Hardening — lint/audit gates, benchmarks, security review

Details: `DESIGN.md`, `docs/`, `docs/superpowers/specs/`.
