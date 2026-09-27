# MCP server

`datara mcp-serve` runs an rmcp server over **stdio only** — no network
transport exists or is planned for the MVP. The server resolves connections
through `datara-storage`, fetches credentials from Secret Service, and calls
the same `DatabaseService`/`DatabaseSession` layer the GUI uses — there is no
second database stack.

Sessions are opened lazily per `connection_id` and cached for the server's
lifetime. `mcp.enabled` (default `false`) and `mcp.max_result_rows`
(default 1000) come from `AppConfig`.

**Implemented:** the `mcp-serve` subcommand exists and exits `2` with a
not-implemented message. **Pending:** rmcp server + tools — phase 7.
