# Query execution

`DatabaseSession::execute(database, query, max_rows)` returns a `QueryResult`
(columns, rows, `rows_affected`, `truncated`). Drivers must stop
materializing at `max_rows` and set `truncated` — the bound is enforced at
the driver, not the UI. Table preview is always `SELECT TOP N` with
`query.default_limit` (1000).

Execution runs on the Tokio runtime; the UI thread only awaits results.
Cancellation flows through `session.cancel()` → TDS attention.
`Query{error_number, line, column}` in `DomainError` preserves server error
positions for editor reporting.

**Implemented:** `QueryResult`/`QueryRow`/`Value` types and the `execute`
signature. **Pending:** execution path end-to-end — phase 4 (task 4.3).
