# MCP tools

Six tools, all taking explicit identifiers — no ambient "current connection":

| Tool | Args | Returns |
|---|---|---|
| `list_connections` | — | stored profiles (no secrets) |
| `list_databases` | `connection_id` | `DatabaseInfo[]` |
| `list_tables` | `connection_id`, `database` | `TableInfo[]` |
| `describe_table` | `connection_id`, `database`, `schema`, `table` | `TableDescription` |
| `search_schema` | `connection_id`, `database`, `pattern` | matching tables/columns |
| `execute_query` | `connection_id`, `database`, `query` | `QueryResult` capped at `mcp.max_result_rows` with `truncated` |

Schemas are declared with `schemars`; results serialize the domain types.
Read vs. write is distinguished so agents can gate mutating calls.

**Implemented:** none — tool surface is the contract. **Pending:** phase 7
(task 7.2).
