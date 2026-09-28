# Datara documentation

Index of the `docs/` tree. Every path below is verified by `make docs-check`.
Top-level docs live in the repo root: `../README.md`, `../DESIGN.md`,
`../AGENTS.md`, `../CONTRIBUTING.md`, `../SECURITY.md`.

## Architecture

- [architecture/overview.md](architecture/overview.md) — workspace layout, dependency direction
- [architecture/application.md](architecture/application.md) — app crate, bridge, command system
- [architecture/database.md](architecture/database.md) — driver/session traits, DatabaseService
- [architecture/ui.md](architecture/ui.md) — Slint structure, theme, threading boundary

## Database

- [database/mssql.md](database/mssql.md) — Tiberius driver, TLS, identifier quoting
- [database/query-execution.md](database/query-execution.md) — execute path, row caps, cancellation
- [database/schema-browser.md](database/schema-browser.md) — schema tree model and preview

## MCP server

- [mcp/overview.md](mcp/overview.md) — stdio server, shared services
- [mcp/tools.md](mcp/tools.md) — tool list and contracts
- [mcp/security.md](mcp/security.md) — credential handling, scoping, limits
- [integrations/mcp.md](integrations/mcp.md) — client setup (Claude Desktop & co.)

## Security

- [security/secrets.md](security/secrets.md) — Secret Service integration, SecretReference
- [security/tls.md](security/tls.md) — encryption modes, certificate policy

## Development

- [development/setup.md](development/setup.md) — toolchain, mise, make targets
- [development/testing.md](development/testing.md) — test layout, container harness
- [development/debugging.md](development/debugging.md) — logging, Wayland notes
- [development/performance.md](development/performance.md) — budgets, benchmarks

## UI

- [ui/components.md](ui/components.md) — Slint component inventory and status

## Packaging

- [packaging/rpm.md](packaging/rpm.md) — Fedora RPM spec
- [packaging/flatpak.md](packaging/flatpak.md) — Flatpak manifest and permissions

## Decisions

- [decisions/ADR-001-rust-and-slint.md](decisions/ADR-001-rust-and-slint.md) — language/UI stack and GPL-3.0
- [decisions/ADR-002-mssql-first.md](decisions/ADR-002-mssql-first.md) — MSSQL-only MVP behind traits
- [decisions/ADR-003-mcp.md](decisions/ADR-003-mcp.md) — MCP over stdio, security posture

## Internal specs

- [superpowers/specs/2026-09-27-datara-design.md](superpowers/specs/2026-09-27-datara-design.md) — locked decisions
- [superpowers/specs/2026-09-27-datara-build-spec.md](superpowers/specs/2026-09-27-datara-build-spec.md) — requirements
- [superpowers/plans/2026-09-27-datara-mvp.md](superpowers/plans/2026-09-27-datara-mvp.md) — phase plan
