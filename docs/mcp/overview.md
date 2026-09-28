# MCP server

`datara mcp-serve` runs an rmcp server over **stdio only** — no network
transport exists or is planned for the MVP. The server resolves connections
through `datara-storage`, fetches credentials from Secret Service, and calls
the same `DatabaseService`/`DatabaseSession` layer the GUI uses — there is no
second database stack.

Sessions are opened lazily per `conn_id` and cached for the server's
lifetime.

## Enabling

MCP is opt-in. Set in `config.toml` (see `docs/mcp/security.md` for why the
defaults are conservative):

```toml
[mcp]
enabled = true          # default false — mcp-serve refuses when unset
max_result_rows = 1000  # row cap per tool call
allow_writes = false    # default false — non-SELECT statements refused
```

When `enabled` is false, `datara mcp-serve` prints a message naming the
config file and exits 0 — it never serves a disabled configuration.

Client setup (Claude Desktop and similar): see
[`docs/integrations/mcp.md`](../integrations/mcp.md).
