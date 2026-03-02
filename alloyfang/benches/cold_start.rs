//! Cold start benchmark: time from module registration to first execution readiness.
//!
//! Compares:
//! - Baseline: all modules compiled eagerly at registration time
//! - AloyFang: modules compiled lazily on first access (AlloyStack on-demand loading)

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use openfang_runtime::alloystack_optim::lazy_loader::LazyWasmLoader;
use std::time::Duration;

/// Minimal valid WASM module binary.
fn empty_wasm() -> Vec<u8> {
    vec![
        0x00, 0x61, 0x73, 0x6D, // magic
        0x01, 0x00, 0x00, 0x00, // version
    ]
}

fn bench_cold_start(c: &mut Criterion) {
    let mut group = c.benchmark_group("cold_start");
    group.measurement_time(Duration::from_secs(10));

    for num_modules in [1, 5, 10, 20] {
        // Baseline: register and preload ALL modules (eager compilation).
        group.bench_with_input(
            BenchmarkId::new("baseline_eager", num_modules),
            &num_modules,
            |b, &n| {
                b.iter(|| {
                    let mut loader = LazyWasmLoader::new(64);
                    for i in 0..n {
                        loader.register_module(&format!("mod_{}", i), empty_wasm());
                    }
                    // Eagerly compile all modules.
                    for i in 0..n {
                        loader.preload(&format!("mod_{}", i)).unwrap();
                    }
                    // Access the first module.
                    let _ = loader.get_or_compile("mod_0").unwrap();
                });
            },
        );

        // AloyFang: register all, but only compile the one actually used (lazy).
        group.bench_with_input(
            BenchmarkId::new("alloyfang_lazy", num_modules),
            &num_modules,
            |b, &n| {
                b.iter(|| {
                    let mut loader = LazyWasmLoader::new(64);
                    for i in 0..n {
                        loader.register_module(&format!("mod_{}", i), empty_wasm());
                    }
                    // Only compile the first module on demand.
                    let _ = loader.get_or_compile("mod_0").unwrap();
                });
            },
        );
    }

    group.finish();
}

fn bench_warm_start(c: &mut Criterion) {
    let mut group = c.benchmark_group("warm_start");
    group.measurement_time(Duration::from_secs(5));

    // Warm start: module already cached, just LRU lookup.
    group.bench_function("cached_module_access", |b| {
        let mut loader = LazyWasmLoader::new(64);
        loader.register_module("cached", empty_wasm());
        let _ = loader.get_or_compile("cached").unwrap(); // Prime the cache.

        b.iter(|| {
            let _ = loader.get_or_compile("cached").unwrap();
        });
    });

    group.finish();
}

criterion_group!(benches, bench_cold_start, bench_warm_start);
criterion_main!(benches);
