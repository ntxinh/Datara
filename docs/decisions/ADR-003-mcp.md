# ADR-003: MCP server over stdio

**Status:** accepted (2026-09-27)

## Context

Agentic clients (Claude Code, etc.) drive tools through MCP. Datara can
expose its connection/query surface so agents can inspect schemas and run
queries against *stored* profiles. Transports: stdio (child process) or
HTTP/SSE (network).

## Decision

`datara mcp-serve` runs rmcp over **stdio only**. The server reuses the
application's `DatabaseService` — the same sessions, credential resolution,
and row caps as the GUI — rather than a parallel stack.

## Security posture

- Stdio means the connecting client is the parent process: no sockets, no
  authn problem, no network exposure. Off-by-default via `mcp.enabled`.
- Tools require explicit `connection_id`/`database`; only stored profiles are
  reachable — the server cannot connect to arbitrary hosts.
- Passwords never cross the MCP boundary; results cap at
  `mcp.max_result_rows` (1000) with truncation reported.

## Consequences

- Adding HTTP transport later would need an auth model — a deliberate new
  ADR, not a flag flip.
- Tool contracts and limits are documented in `docs/mcp/`.
