//! Agent loop benchmark: simulates a full agent turn with mock LLM and tools.
//!
//! Compares baseline (sequential tools, JSON data passing) vs AloyFang
//! (parallel tools, zero-copy data passing).

use alloyfang::mock_llm::MockLlmDriver;
use bytes::Bytes;
use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use openfang_runtime::alloystack_optim::parallel_executor::ParallelWorkflowExecutor;
use openfang_runtime::alloystack_optim::reference_passing::SharedBufferPool;
use std::time::Duration;

/// Simulate a baseline agent turn: LLM call -> sequential tool execution -> data via JSON.
async fn baseline_agent_turn(
    llm: &MockLlmDriver,
    num_tools: usize,
    tool_latency: Duration,
    data_size: usize,
) -> Duration {
    let start = std::time::Instant::now();

    // 1. LLM call.
    let _response = llm.complete().await;

    // 2. Sequential tool execution.
    for _ in 0..num_tools {
        tokio::time::sleep(tool_latency).await;

        // 3. Data passing via JSON serialization.
        let data = vec![42u8; data_size];
        let serialized = serde_json::to_vec(&data).unwrap();
        let _deserialized: Vec<u8> = serde_json::from_slice(&serialized).unwrap();
    }

    start.elapsed()
}

/// Simulate an AloyFang agent turn: LLM call -> parallel tool execution -> zero-copy data.
async fn alloyfang_agent_turn(
    llm: &MockLlmDriver,
    executor: &ParallelWorkflowExecutor,
    pool: &SharedBufferPool,
    num_tools: usize,
    tool_latency: Duration,
    data_size: usize,
) -> Duration {
    let start = std::time::Instant::now();

    // 1. LLM call.
    let _response = llm.complete().await;

    // 2. Parallel tool execution.
    let tasks: Vec<_> = (0..num_tools)
        .map(|i| {
            let id = format!("tool_{}", i);
            let fut = async move {
                tokio::time::sleep(tool_latency).await;
                Ok::<_, anyhow::Error>(i)
            };
            (id, fut)
        })
        .collect();
    let _ = executor.execute_group(tasks).await;

    // 3. Data passing via shared buffer (zero-copy).
    for i in 0..num_tools {
        let data = Bytes::from(vec![42u8; data_size]);
        pool.put(&format!("tool_{}_output", i), data, i as u64)
            .unwrap();
    }
    for i in 0..num_tools {
        let _ = pool.take(&format!("tool_{}_output", i));
    }

    start.elapsed()
}

fn bench_agent_turn(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let mut group = c.benchmark_group("agent_turn");
    group.measurement_time(Duration::from_secs(20));
    group.sample_size(30);

    let llm_latency = Duration::from_millis(20);
    let tool_latency = Duration::from_millis(10);
    let data_size = 10_000; // 10KB per tool output

    for num_tools in [1, 3, 5] {
        // Baseline: sequential tools, JSON data.
        group.bench_with_input(
            BenchmarkId::new("baseline", num_tools),
            &num_tools,
            |b, &n| {
                let llm = MockLlmDriver::text_only("response", llm_latency);
                b.iter(|| {
                    rt.block_on(baseline_agent_turn(&llm, n, tool_latency, data_size));
                });
            },
        );

        // AloyFang: parallel tools, zero-copy data.
        group.bench_with_input(
            BenchmarkId::new("alloyfang", num_tools),
            &num_tools,
            |b, &n| {
                let llm = MockLlmDriver::text_only("response", llm_latency);
                let executor = ParallelWorkflowExecutor::new(num_cpus::get());
                let pool = SharedBufferPool::new(n * data_size * 2);
                b.iter(|| {
                    rt.block_on(alloyfang_agent_turn(
                        &llm,
                        &executor,
                        &pool,
                        n,
                        tool_latency,
                        data_size,
                    ));
                });
            },
        );
    }

    group.finish();
}

fn bench_multi_turn_conversation(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let mut group = c.benchmark_group("multi_turn_conversation");
    group.measurement_time(Duration::from_secs(20));
    group.sample_size(20);

    let llm_latency = Duration::from_millis(15);
    let tool_latency = Duration::from_millis(5);
    let data_size = 5_000;
    let num_turns = 5;
    let tools_per_turn = 3;

    group.bench_function("baseline_5_turns", |b| {
        let llm = MockLlmDriver::text_only("response", llm_latency);
        b.iter(|| {
            rt.block_on(async {
                for _ in 0..num_turns {
                    baseline_agent_turn(&llm, tools_per_turn, tool_latency, data_size).await;
                }
            });
        });
    });

    group.bench_function("alloyfang_5_turns", |b| {
        let llm = MockLlmDriver::text_only("response", llm_latency);
        let executor = ParallelWorkflowExecutor::new(num_cpus::get());
        let pool = SharedBufferPool::new(tools_per_turn * data_size * 2);
        b.iter(|| {
            rt.block_on(async {
                for _ in 0..num_turns {
                    alloyfang_agent_turn(
                        &llm,
                        &executor,
                        &pool,
                        tools_per_turn,
                        tool_latency,
                        data_size,
                    )
                    .await;
                }
            });
        });
    });

    group.finish();
}

criterion_group!(benches, bench_agent_turn, bench_multi_turn_conversation);
criterion_main!(benches);
