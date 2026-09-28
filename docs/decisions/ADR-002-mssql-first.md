# ADR-002: MSSQL first (and only, for the MVP)

**Status:** accepted (2026-09-27)

## Context

A multi-database client needs a driver abstraction regardless. Supporting
PostgreSQL and SQLite alongside MSSQL would roughly triple driver,
conversion, and test surface for the MVP.

## Decision

MVP ships **MSSQL only**, via Tiberius over TDS 7.3 with rustls. The
`DatabaseDriver`/`DatabaseSession` traits in `datara-database` are the
seam — PostgreSQL/SQLite drivers are *designed-for* (the traits are
deliberately not MSSQL-shaped where avoidable) but no other driver code is
written yet.

## Consequences

- `list_*` signatures take an explicit `database` because MSSQL metadata
  needs database context — a shape other drivers can satisfy too.
- `quote_ident` is per-driver (`[brackets]` for MSSQL); no shared quoting.
- Adding a second driver means one new `datara-driver-*` crate and
  registration in `app` — no changes to UI, storage, or MCP.
- Explicit rule: do not stub Postgres/SQLite drivers "for later" (AGENTS.md).
