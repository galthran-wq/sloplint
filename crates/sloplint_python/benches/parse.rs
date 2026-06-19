//! Parser throughput benchmark — the speed gate.
//!
//! sloplint's whole premise is being Ruff-fast, and parsing dominates per-file cost. This
//! pins a baseline so a future change that regresses throughput is visible. Run with
//! `cargo bench -p sloplint_python`; CI compiles it (`--no-run`) to keep it from rotting.

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use sloplint_python::parse;

fn synthetic_module(function_count: usize) -> String {
    let mut source = String::new();
    for i in 0..function_count {
        source.push_str(&format!(
            "def func_{i}(a, b):\n    total = a + b\n    return total\n\n"
        ));
    }
    source
}

fn bench_parse(c: &mut Criterion) {
    let source = synthetic_module(200);
    c.bench_function("parse_200_functions", |b| {
        b.iter(|| {
            let parsed = parse(black_box(&source)).expect("parses");
            black_box(parsed.syntax().body.len())
        });
    });
}

criterion_group!(benches, bench_parse);
criterion_main!(benches);
