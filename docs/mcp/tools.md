# MCP tools

Six tools, all taking explicit identifiers — no ambient "current connection".
`conn_id` values come from `list_connections`. `database`/`schema` arguments
are optional where noted: omitted `database` falls back to the profile's
configured database, then `master`; omitted `schema` defaults to `dbo`.

| Tool | Args | Returns |
|---|---|---|
| `list_connections` | — | `{connections: [{id,name,host,port,database,username}], truncated}` — no secrets |
| `list_databases` | `conn_id` | `{databases: [{name}], truncated}` |
| `list_tables` | `conn_id`, `database?`, `schema?` | `{tables: [{schema,name,kind}], truncated}` — `kind` is `"table"`/`"view"` |
| `describe_table` | `conn_id`, `table`, `database?`, `schema?` | `{table: {schema,name,columns[],indexes[]}, truncated}` |
| `search_schema` | `conn_id`, `query`, `database?` | `{tables: [{schema,name,kind}], truncated}` — LIKE wildcards `%`/`_` work |
| `execute_query` | `conn_id`, `query`, `database?` | `{statement_type, columns, rows, rows_affected, truncated}` |

`search_schema`'s `query` is a name fragment matched case-insensitively via
SQL `LIKE`; `[` is escaped, `'` is safe.

## `execute_query` policy

Each result reports `statement_type`: `"read"` when every statement in the
batch parses to a `SELECT`, `"write"` otherwise. With `mcp.allow_writes =
false` (the default) write statements — and SQL the parser cannot read —
are refused with a tool error naming the config key to change.

Rows are capped at `mcp.max_result_rows` (default 1000); every list-shaped
result carries a `truncated` boolean so a capped answer never looks complete.

The classification walks the full query tree — `SELECT INTO`, CTE bodies,
UNION arms, and derived tables are checked, not just the top-level keyword.
Table-valued functions (`OPENQUERY`, `OPENROWSET`, `OPENXML`, `UNNEST`,
`JSON_TABLE`, …) are treated as **not read**: they can execute remote DML or
carry connection strings, so `allow_writes = false` refuses them. SQL the
parser cannot read is likewise refused — the parser never vouches for what
it couldn't parse. (Expression-level subqueries in `WHERE`/select lists are
not walked; T-SQL forbids writes there anyway, so no bypass exists.)

Errors surface as tool-level error content (`isError: true`) rather than
JSON-RPC errors, so the model can read the message and react.
