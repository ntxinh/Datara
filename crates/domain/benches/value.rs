//! Baseline for `Value::Display` — the per-cell formatting the grid runs for
//! every materialized row.

use std::hint::black_box;

use criterion::{criterion_group, criterion_main, Criterion};
use datara_domain::Value;

fn benches(c: &mut Criterion) {
    let values = [
        Value::Null,
        Value::Bool(true),
        Value::Int(-42),
        Value::Float(42.125),
        Value::Decimal("1234567890.12345".into()),
        Value::Text("the quick brown fox".into()),
        Value::Bytes(vec![0xDE, 0xAD, 0xBE, 0xEF]),
        Value::DateTime("2026-09-28T12:34:56Z".into()),
        Value::Uuid(uuid::Uuid::nil()),
    ];
    c.bench_function("Value::Display mixed x9000", |b| {
        b.iter(|| {
            for _ in 0..1000 {
                for v in &values {
                    black_box(v.to_string());
                }
            }
        })
    });
}

criterion_group!(domain, benches);
criterion_main!(domain);
