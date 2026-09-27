# Schema browser

The sidebar tree is a flat model (`databases → schemas → tables → columns`)
backed by the session `list_*` calls, refreshed on demand — not a live
subscription. Node kinds come from `TableInfo.kind` (`Base` | `View`); column
nodes show `data_type`, `nullable`, and primary-key markers from
`ColumnInfo`/`TableDescription`.

Selecting a table issues a bounded preview (`SELECT TOP N`) into the result
grid — never an unbounded scan.

**Implemented:** the metadata types (`DatabaseInfo`, `SchemaInfo`,
`TableInfo`, `ColumnInfo`, `TableDescription`, `IndexInfo`). **Pending:** the
tree model and wiring — phase 3 (tasks 3.1–3.2).
