# Performance

Budgets and the principles behind them:

- UI thread never blocks — all DB I/O on Tokio, results posted via
  `invoke_from_event_loop`.
- Grid virtualizes rows: only the viewport is materialized for Slint, backed
  by `RowCache`; tens of thousands of rows must stay responsive.
- Drivers cap result materialization at `max_rows`; previews use
  `SELECT TOP N`.
- Criterion benchmarks (startup, schema load, result processing, grid
  scroll) arrive in phase 9 and will pin these targets numerically.

**Implemented:** none — no benchmarks yet. **Pending:** phase 9 (task 9.2).
