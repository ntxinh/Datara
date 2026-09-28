# MCP client integration

How to connect an MCP-capable client (Claude Desktop, Claude Code, any stdio
client) to Datara's embedded server. Tool contracts live in
[`docs/mcp/tools.md`](../mcp/tools.md); the security posture in
[`docs/mcp/security.md`](../mcp/security.md).

## 1. Enable MCP

```toml
# ~/.config/io.github.ntxinh.Datara/config.toml
[mcp]
enabled = true
# max_result_rows = 1000  # optional
# allow_writes = false    # optional; true lets execute_query run non-SELECT
```

Without `enabled = true`, `datara mcp-serve` prints a hint naming this file
and exits 0.

## 2. Register the server

Claude Desktop (`claude_desktop_config.json`) or any client taking a
command-line server:

```json
{
  "mcpServers": {
    "datara": {
      "command": "datara",
      "args": ["mcp-serve"]
    }
  }
}
```

If `datara` is not on the client's `PATH`, use the absolute path (e.g.
`"/usr/bin/datara"` or `"/home/<you>/.cargo/bin/datara"`).

Claude Code one-liner:

```sh
claude mcp add datara -- datara mcp-serve
```

## 3. Security notes

- The client launches `datara mcp-serve` over stdio — there is no port to
  expose and no auth to configure; whoever can run the binary as you can use
  your stored profiles.
- Tools only reach **saved connection profiles** (`conn_id`); they cannot
  dial arbitrary hosts or accept connection strings.
- No password or secret reference can appear in a tool response.
- Writes are refused unless `allow_writes = true`; keep it `false` for
  read-only agent workflows.
