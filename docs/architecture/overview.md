# Architecture overview

Ten-crate Cargo workspace; dependency direction is `UI → app → domain →
infrastructure → drivers`. `datara-domain` holds types only and depends on
nothing project-internal; `datara-app` is the sole Slint consumer and the
composition root. See `DESIGN.md` for the system diagram and crate graph.

**Implemented:** workspace scaffold, `domain` types, `config`, `storage`,
`database` traits, `sql-editor` (statement splitting, highlighting,
completions), `app` GUI (sidebar, tabbed query editor with highlight overlay
+ completions, command dispatch, status bar) + `mcp-serve` stub.
**Pending:** driver-mssql, secrets, data-grid, mcp-server bodies —
phases 2–7.
