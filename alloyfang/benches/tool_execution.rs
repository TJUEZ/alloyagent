//! Tool execution benchmark: measures dispatch overhead for different tool types.

use criterion::{criterion_group, criterion_main, Criterion};
use alloyfang::mock_tools;
use std::time::Duration;

fn bench_tool_dispatch(c: &mut Criterion) {
    let mut group = c.benchmark_group("tool_dispatch");
    group.measurement_time(Duration::from_secs(5));

    // Measure pure dispatch overhead (no-op tool).
    group.bench_function("noop_tool", |b| {
        b.iter(|| mock_tools::noop_tool());
    });

    // Measure CPU-bound tool at different intensities.
    for iterations in [1_000, 10_000, 100_000, 1_000_000] {
        group.bench_function(format!("cpu_bound_{}", iterations), |b| {
            b.iter(|| mock_tools::cpu_bound_tool(iterations));
        });
    }

    // Measure large output generation.
    for size in [1_000, 10_000, 100_000, 1_000_000] {
        group.bench_function(format!("large_output_{}", size), |b| {
            b.iter(|| mock_tools::large_output_tool(size));
        });
    }

    group.finish();
}

fn bench_tool_chain(c: &mut Criterion) {
    let mut group = c.benchmark_group("tool_chain");
    group.measurement_time(Duration::from_secs(5));

    for steps in [3, 5, 10] {
        group.bench_function(format!("chain_{}_steps_1kb", steps), |b| {
            b.iter(|| mock_tools::tool_chain(steps, 1_000));
        });

        group.bench_function(format!("chain_{}_steps_100kb", steps), |b| {
            b.iter(|| mock_tools::tool_chain(steps, 100_000));
        });
    }

    group.finish();
}

criterion_group!(benches, bench_tool_dispatch, bench_tool_chain);
criterion_main!(benches);
