//! Baselines for the editor hot paths: statement splitting and highlight
//! tokenization over a realistic editor document.

use std::hint::black_box;

use criterion::{criterion_group, criterion_main, Criterion};
use datara_sql_editor::{highlight, split_statements};

/// ~100-statement, ~10KB T-SQL document. Deterministic — no fixtures.
fn bench_doc() -> String {
    let mut doc = String::new();
    for i in 0..100 {
        doc.push_str(&format!("-- stmt {i}: report\n"));
        match i % 4 {
            0 => doc.push_str(&format!(
                "SELECT t.id, t.name, t.score FROM dbo.users_{i} AS t WHERE t.score >= {} AND t.name LIKE N'user_%';\n",
                i * 37
            )),
            1 => doc.push_str(&format!(
                "UPDATE dbo.orders SET status = 'shipped;done', updated_at = GETDATE() WHERE id = {i};\n"
            )),
            2 => doc.push_str(&format!(
                "INSERT INTO dbo.log_{i} (msg, lvl) VALUES ('row {i}', 3);\n"
            )),
            _ => doc.push_str(&format!(
                "DELETE FROM dbo.tmp_{i} WHERE flag = 0 -- cleanup\nAND created < DATEADD(day, -30, SYSDATETIME());\n"
            )),
        }
    }
    doc
}

fn benches(c: &mut Criterion) {
    let doc = bench_doc();
    c.bench_function("split_statements 100-stmt ~10KB", |b| {
        b.iter(|| split_statements(black_box(&doc)))
    });
    c.bench_function("highlight 100-stmt ~10KB", |b| {
        b.iter(|| highlight(black_box(&doc)))
    });
}

criterion_group!(sql_editor, benches);
criterion_main!(sql_editor);
