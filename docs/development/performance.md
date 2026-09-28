# Performance

Budgets and the principles behind them:

- UI thread never blocks — all DB I/O on Tokio, results posted via
  `invoke_from_event_loop`.
- Grid virtualizes rows: only the viewport is materialized for Slint, backed
  by `RowCache`; tens of thousands of rows must stay responsive.
- Drivers cap result materialization at `max_rows`; previews use
  `SELECT TOP N`.

## Baselines (task 9.2)

Criterion 0.7 benches live in `crates/sql-editor/benches/statements.rs`,
`crates/data-grid/benches/cache.rs`, and `crates/domain/benches/value.rs`.
Run: `cargo +1.98.1 bench -p datara-sql-editor -p datara-data-grid -p datara-domain`.

Environment: Intel Core i5-10400H (4C/8T, 2.60GHz), x86_64 Fedora 44,
rustc 1.98.1 (stable pin), release profile. Run date: 2026-09-28.

| Metric | Result | Notes |
|---|---|---|
| `split_statements`, 100-statement ~10KB doc | 22.2 µs | per full-document split |
| `highlight`, same doc | 86.7 µs | per full-document tokenize |
| `RowCache::from_result`, 10k rows × 20 cols | ~59 ns | move-only; real cost is upstream materialization |
| `row_data` × 64 (one viewport paint) | 96.6 µs | ~1.5 µs/row incl. `cell_text` formatting |
| `Value::Display`, 9000 mixed-type cells | 403.8 µs | ~45 ns/cell |
| `datara --version` startup | 21–23 ms real | `time` ×3, warm cache; covers binary load + clap parse only — GUI first-paint can't be headless-timed, measure interactively if it matters |

Baselines only — no CI regression gates. ponytail: add bench comparison to
CI when a perf complaint or drift actually shows up.
