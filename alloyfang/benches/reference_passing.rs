//! Reference passing benchmark: JSON serialization vs zero-copy shared buffers.
//!
//! Compares:
//! - Baseline: serialize data to JSON, deserialize on the other end (traditional approach)
//! - AloyFang: put/take via SharedBufferPool (zero-copy via Bytes refcounting)
//!
//! This directly tests the benefit inspired by AlloyStack's faas_buffer.

use bytes::Bytes;
use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use openfang_runtime::alloystack_optim::reference_passing::SharedBufferPool;
use std::time::Duration;

fn bench_data_transfer(c: &mut Criterion) {
    let mut group = c.benchmark_group("data_transfer");
    group.measurement_time(Duration::from_secs(10));

    for size in [1_000, 10_000, 100_000, 1_000_000, 10_000_000] {
        group.throughput(Throughput::Bytes(size as u64));

        // Baseline: JSON round-trip (serialize + deserialize).
        group.bench_with_input(
            BenchmarkId::new("json_roundtrip", size),
            &size,
            |b, &sz| {
                let data = vec![42u8; sz];
                b.iter(|| {
                    let serialized = serde_json::to_vec(&data).unwrap();
                    let _deserialized: Vec<u8> = serde_json::from_slice(&serialized).unwrap();
                });
            },
        );

        // AloyFang: SharedBufferPool put + take (zero-copy).
        group.bench_with_input(
            BenchmarkId::new("shared_buffer", size),
            &size,
            |b, &sz| {
                let pool = SharedBufferPool::new(sz * 2);
                let data = Bytes::from(vec![42u8; sz]);
                b.iter(|| {
                    pool.put("transfer_slot", data.clone(), 0).unwrap();
                    let _ = pool.take("transfer_slot").unwrap();
                });
            },
        );

        // AloyFang: SharedBufferPool put + get (multi-consumer read).
        group.bench_with_input(
            BenchmarkId::new("shared_buffer_read", size),
            &size,
            |b, &sz| {
                let pool = SharedBufferPool::new(sz * 2);
                let data = Bytes::from(vec![42u8; sz]);
                pool.put("read_slot", data, 0).unwrap();
                b.iter(|| {
                    let _ = pool.get("read_slot").unwrap();
                });
            },
        );
    }

    group.finish();
}

fn bench_multi_slot_throughput(c: &mut Criterion) {
    let mut group = c.benchmark_group("multi_slot_throughput");
    group.measurement_time(Duration::from_secs(10));

    let slot_count = 50;
    let slot_size = 10_000; // 10KB per slot

    // Baseline: Vec<String> of JSON blobs.
    group.bench_function("json_vec_store_retrieve", |b| {
        b.iter(|| {
            let mut store: Vec<(String, Vec<u8>)> = Vec::with_capacity(slot_count);
            let data = vec![42u8; slot_size];
            for i in 0..slot_count {
                let serialized = serde_json::to_vec(&data).unwrap();
                store.push((format!("slot_{}", i), serialized));
            }
            // Retrieve all.
            for (_, blob) in &store {
                let _: Vec<u8> = serde_json::from_slice(blob).unwrap();
            }
        });
    });

    // AloyFang: SharedBufferPool.
    group.bench_function("buffer_pool_store_retrieve", |b| {
        b.iter(|| {
            let pool = SharedBufferPool::new(slot_count * slot_size * 2);
            let data = Bytes::from(vec![42u8; slot_size]);
            for i in 0..slot_count {
                pool.put(&format!("slot_{}", i), data.clone(), 0).unwrap();
            }
            // Retrieve all.
            for i in 0..slot_count {
                let _ = pool.get(&format!("slot_{}", i)).unwrap();
            }
        });
    });

    group.finish();
}

criterion_group!(benches, bench_data_transfer, bench_multi_slot_throughput);
criterion_main!(benches);
