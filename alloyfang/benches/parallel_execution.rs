//! Parallel execution benchmark: sequential vs parallel task execution.
//!
//! Compares:
//! - Baseline: tasks executed sequentially (one after another)
//! - AloyFang: tasks executed in parallel using ParallelWorkflowExecutor
//!
//! Inspired by AlloyStack's `run_group_in_parallel()`.

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use openfang_runtime::alloystack_optim::parallel_executor::{
    ParallelWorkflowExecutor, WorkItem,
};
use std::time::Duration;

fn bench_io_bound_parallel(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let mut group = c.benchmark_group("parallel_io_bound");
    group.measurement_time(Duration::from_secs(15));

    let task_sleep_ms = 10u64;

    for num_tasks in [2, 4, 8, 16] {
        // Baseline: sequential execution.
        group.bench_with_input(
            BenchmarkId::new("sequential", num_tasks),
            &num_tasks,
            |b, &n| {
                b.iter(|| {
                    rt.block_on(async {
                        for _ in 0..n {
                            tokio::time::sleep(Duration::from_millis(task_sleep_ms)).await;
                        }
                    });
                });
            },
        );

        // AloyFang: parallel execution.
        group.bench_with_input(
            BenchmarkId::new("parallel", num_tasks),
            &num_tasks,
            |b, &n| {
                let executor = ParallelWorkflowExecutor::new(num_cpus::get());
                b.iter(|| {
                    rt.block_on(async {
                        let tasks: Vec<_> = (0..n)
                            .map(|i| {
                                let id = format!("task_{}", i);
                                let fut = async move {
                                    tokio::time::sleep(Duration::from_millis(task_sleep_ms))
                                        .await;
                                    Ok::<_, anyhow::Error>(i)
                                };
                                (id, fut)
                            })
                            .collect();
                        let _ = executor.execute_group(tasks).await;
                    });
                });
            },
        );
    }

    group.finish();
}

fn bench_cpu_bound_parallel(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let mut group = c.benchmark_group("parallel_cpu_bound");
    group.measurement_time(Duration::from_secs(15));

    let work_iterations = 1_000_000usize;

    for num_tasks in [2, 4, 8] {
        // Baseline: sequential CPU work.
        group.bench_with_input(
            BenchmarkId::new("sequential", num_tasks),
            &num_tasks,
            |b, &n| {
                b.iter(|| {
                    for _ in 0..n {
                        let mut sum = 0u64;
                        for i in 0..work_iterations {
                            sum = sum.wrapping_add(i as u64);
                            std::hint::black_box(sum);
                        }
                    }
                });
            },
        );

        // AloyFang: parallel CPU work via spawn_blocking.
        group.bench_with_input(
            BenchmarkId::new("parallel", num_tasks),
            &num_tasks,
            |b, &n| {
                let executor = ParallelWorkflowExecutor::new(num_cpus::get());
                b.iter(|| {
                    rt.block_on(async {
                        let tasks: Vec<_> = (0..n)
                            .map(|i| {
                                let id = format!("task_{}", i);
                                let fut = async move {
                                    tokio::task::spawn_blocking(move || {
                                        let mut sum = 0u64;
                                        for j in 0..work_iterations {
                                            sum = sum.wrapping_add(j as u64);
                                            std::hint::black_box(sum);
                                        }
                                        sum
                                    })
                                    .await
                                    .map_err(|e| anyhow::anyhow!("{}", e))
                                };
                                (id, fut)
                            })
                            .collect();
                        let _ = executor.execute_group(tasks).await;
                    });
                });
            },
        );
    }

    group.finish();
}

fn bench_dag_execution(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let mut group = c.benchmark_group("dag_execution");
    group.measurement_time(Duration::from_secs(10));

    // Diamond DAG: A -> (B, C) -> D
    group.bench_function("diamond_dag", |b| {
        let executor = ParallelWorkflowExecutor::new(num_cpus::get());
        b.iter(|| {
            rt.block_on(async {
                let items = vec![
                    WorkItem { id: "a".into(), dependencies: vec![] },
                    WorkItem { id: "b".into(), dependencies: vec!["a".into()] },
                    WorkItem { id: "c".into(), dependencies: vec!["a".into()] },
                    WorkItem { id: "d".into(), dependencies: vec!["b".into(), "c".into()] },
                ];
                let _ = executor
                    .execute_dag(items, |_id| async {
                        tokio::time::sleep(Duration::from_millis(5)).await;
                        Ok::<_, anyhow::Error>(())
                    })
                    .await;
            });
        });
    });

    // Wide fan-out: A -> (B1, B2, B3, B4, B5, B6, B7, B8) -> C
    group.bench_function("wide_fanout_8", |b| {
        let executor = ParallelWorkflowExecutor::new(num_cpus::get());
        b.iter(|| {
            rt.block_on(async {
                let mut items = vec![WorkItem { id: "start".into(), dependencies: vec![] }];
                for i in 0..8 {
                    items.push(WorkItem {
                        id: format!("worker_{}", i),
                        dependencies: vec!["start".into()],
                    });
                }
                items.push(WorkItem {
                    id: "end".into(),
                    dependencies: (0..8).map(|i| format!("worker_{}", i)).collect(),
                });

                let _ = executor
                    .execute_dag(items, |_id| async {
                        tokio::time::sleep(Duration::from_millis(5)).await;
                        Ok::<_, anyhow::Error>(())
                    })
                    .await;
            });
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_io_bound_parallel,
    bench_cpu_bound_parallel,
    bench_dag_execution
);
criterion_main!(benches);
