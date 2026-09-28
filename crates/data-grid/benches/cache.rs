//! Baselines for grid materialization: building a `RowCache` and the
//! per-viewport `row_data` calls the `ListView` makes while scrolling.

use std::hint::black_box;
use std::rc::Rc;

use criterion::{criterion_group, criterion_main, BatchSize, Criterion};

use datara_data_grid::{RowCache, VirtualizedRows};
use datara_domain::{QueryColumn, QueryResult, QueryRow, Value};
use slint::Model;

/// 10k rows x 20 cols of mixed-type values, deterministic.
fn sample_result() -> QueryResult {
    let columns = (0..20)
        .map(|c| QueryColumn {
            name: format!("col_{c}"),
            data_type: "int".into(),
        })
        .collect();
    let rows = (0..10_000)
        .map(|r| QueryRow {
            cells: (0..20)
                .map(|c| match (r + c) % 5 {
                    0 => Value::Null,
                    1 => Value::Int((r * 20 + c) as i64),
                    2 => Value::Float(r as f64 + f64::from(c) / 100.0),
                    3 => Value::Text(format!("cell-{r}-{c}")),
                    _ => Value::Bool(r % 2 == 0),
                })
                .collect(),
        })
        .collect();
    QueryResult {
        columns,
        rows,
        rows_affected: None,
        truncated: false,
    }
}

fn benches(c: &mut Criterion) {
    c.bench_function("RowCache::from_result 10k x 20", |b| {
        b.iter_batched(
            sample_result,
            |r| RowCache::from_result(black_box(r)),
            BatchSize::LargeInput,
        )
    });

    let model = VirtualizedRows::new(Rc::new(RowCache::from_result(sample_result())));
    c.bench_function("row_data 64-row viewport", |b| {
        b.iter(|| {
            for row in 0..64 {
                black_box(model.row_data(black_box(row)));
            }
        })
    });
}

criterion_group!(data_grid, benches);
criterion_main!(data_grid);
