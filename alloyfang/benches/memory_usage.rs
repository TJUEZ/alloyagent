//! Memory usage benchmark: RSS tracking across different optimization modes.
//!
//! Measures memory consumption at various stages:
//! - After runtime initialization
//! - After registering N modules (lazy vs eager)
//! - After processing data through shared buffers

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use alloyfang::get_rss_kb;
use bytes::Bytes;
use openfang_runtime::alloystack_optim::lazy_loader::LazyWasmLoader;
use openfang_runtime::alloystack_optim::reference_passing::SharedBufferPool;
use std::time::Duration;

fn empty_wasm() -> Vec<u8> {
    vec![0x00, 0x61, 0x73, 0x6D, 0x01, 0x00, 0x00, 0x00]
}

fn bench_memory_lazy_vs_eager(c: &mut Criterion) {
    let mut group = c.benchmark_group("memory_module_loading");
    group.measurement_time(Duration::from_secs(10));

    for num_modules in [10, 25, 50] {
        // Baseline: eagerly compile all modules, measure peak memory.
        group.bench_with_input(
            BenchmarkId::new("eager_compile_all", num_modules),
            &num_modules,
            |b, &n| {
                b.iter(|| {
                    let rss_before = get_rss_kb();
                    let mut loader = LazyWasmLoader::new(n + 1);
                    for i in 0..n {
                        loader.register_module(&format!("mod_{}", i), empty_wasm());
                        loader.preload(&format!("mod_{}", i)).unwrap();
                    }
                    let rss_after = get_rss_kb();
                    (rss_after as i64 - rss_before as i64, loader.cached_count())
                });
            },
        );

        // AloyFang: register but don't compile, measure memory footprint.
        group.bench_with_input(
            BenchmarkId::new("lazy_register_only", num_modules),
            &num_modules,
            |b, &n| {
                b.iter(|| {
                    let rss_before = get_rss_kb();
                    let mut loader = LazyWasmLoader::new(n + 1);
                    for i in 0..n {
                        loader.register_module(&format!("mod_{}", i), empty_wasm());
                    }
                    // Compile only 1 module.
                    let _ = loader.get_or_compile("mod_0").unwrap();
                    let rss_after = get_rss_kb();
                    (rss_after as i64 - rss_before as i64, loader.cached_count())
                });
            },
        );
    }

    group.finish();
}

fn bench_memory_buffer_pool(c: &mut Criterion) {
    let mut group = c.benchmark_group("memory_buffer_pool");
    group.measurement_time(Duration::from_secs(10));

    for num_buffers in [10, 50, 100] {
        let buffer_size = 10_000; // 10KB each

        group.bench_with_input(
            BenchmarkId::new("shared_buffer_pool", num_buffers),
            &num_buffers,
            |b, &n| {
                b.iter(|| {
                    let pool = SharedBufferPool::new(n * buffer_size * 2);
                    for i in 0..n {
                        let data = Bytes::from(vec![0u8; buffer_size]);
                        pool.put(&format!("buf_{}", i), data, i as u64).unwrap();
                    }
                    pool.current_usage_bytes()
                });
            },
        );

        group.bench_with_input(
            BenchmarkId::new("json_clone_equivalent", num_buffers),
            &num_buffers,
            |b, &n| {
                b.iter(|| {
                    let mut store: Vec<String> = Vec::with_capacity(n);
                    for _ in 0..n {
                        let data = serde_json::to_string(&vec![0u8; buffer_size]).unwrap();
                        store.push(data);
                    }
                    store.len()
                });
            },
        );
    }

    group.finish();
}

criterion_group!(benches, bench_memory_lazy_vs_eager, bench_memory_buffer_pool);
criterion_main!(benches);
