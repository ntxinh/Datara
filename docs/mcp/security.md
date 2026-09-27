# MCP security

- **Stdio only.** No listening socket; the client process launches
  `datara mcp-serve` and owns the pipe. Network exposure is out of scope.
- **No credential exposure.** Tools return profile metadata — the
  `SecretReference` string is never emitted and passwords never leave Secret
  Service except to the session that needs them.
- **Explicit scoping.** Every tool requires `connection_id`; `execute_query`
  also requires `database`. The server cannot reach arbitrary hosts — only
  stored profiles.
- **Bounded results.** `execute_query` enforces `mcp.max_result_rows`
  (default 1000) and reports truncation.
- **Opt-in.** `mcp.enabled` defaults to `false` in config.

**Status:** posture designed; server pending — phase 7.
