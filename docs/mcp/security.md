# MCP security

- **Stdio only.** No listening socket; the client process launches
  `datara mcp-serve` and owns the pipe. Network exposure is out of scope.
- **No credential exposure.** `list_connections` emits a curated summary —
  `secret_reference` and every auth/encryption internal are structurally
  absent, not just redacted. Passwords never leave Secret Service except to
  the session that needs them, and no tool response can carry one.
- **Stored profiles only.** Tools address connections by `conn_id`; there is
  no argument that accepts a host, port, or connection string, so the server
  cannot be steered to arbitrary hosts.
- **Explicit scoping.** Every tool requires `conn_id`; `execute_query` runs
  against the named database (or the profile default). The server cannot
  "execute on an unspecified connection."
- **Read by default.** `execute_query` classifies each batch with sqlparser;
  `mcp.allow_writes` (default `false`) refuses anything not provably a
  `SELECT`, including unparseable SQL. Destructive queries require explicit
  opt-in.
- **Bounded results.** `mcp.max_result_rows` (default 1000) caps every
  row-producing tool; a `truncated` flag marks capped results.
- **Opt-in.** `mcp.enabled` defaults to `false`; `mcp-serve` refuses cleanly
  until the user sets it.

TLS behavior follows the profile's `encryption`/`trust_server_certificate`
settings — MCP never disables verification beyond what the stored profile
already specifies.
