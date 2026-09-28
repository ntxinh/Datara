# Database architecture

`datara-database` defines the seam: `DatabaseDriver::connect(profile,
credentials)` returns a boxed `DatabaseSession` with `list_databases`,
`list_schemas`, `list_tables`, `describe_table`, `execute(database, query,
max_rows)`, `cancel`, and `quote_ident`. `list_*` takes a database because
MSSQL metadata queries need database context; `execute` takes `max_rows` so
drivers cap materialization (spec §11).

`DatabaseService` (planned, phase 2) will own the session map, resolve
credentials from Secret Service, and enforce cancellation.
**Implemented:** both traits. **Pending:** service and the Tiberius driver.
