//! Parallel workflow executor with dependency-aware scheduling.
//!
//! Inspired by AlloyStack's `run_group_in_parallel()` in
//! `libasvisor/src/isolation/mod.rs:199-224`, which uses `thread::scope` to run
//! a group of apps concurrently within the same isolation domain, joining all
//! handles before proceeding to the next group.
//!
//! This module adapts the pattern to tokio's async runtime using `JoinSet` and
//! `Semaphore` for bounded concurrency.

use std::collections::{HashMap, HashSet, VecDeque};
use std::future::Future;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::Semaphore;
use tokio::task::JoinSet;

/// A unit of work in a workflow DAG.
#[derive(Debug, Clone)]
pub struct WorkItem {
    /// Unique identifier for this item.
    pub id: String,
    /// IDs of items that must complete before this one can start.
    pub dependencies: Vec<String>,
}

/// Result of executing a group of parallel tasks.
#[derive(Debug)]
pub struct GroupResult<T> {
    /// Individual results, keyed by task id.
    pub results: Vec<(String, Result<T, anyhow::Error>)>,
    /// Wall-clock time for the entire group.
    pub wall_time: Duration,
    /// Sum of individual task durations (CPU time).
    pub cpu_time: Duration,
    /// Parallelism ratio: cpu_time / wall_time. > 1.0 means parallelism helped.
    pub parallelism_ratio: f64,
}

/// Cumulative statistics for the executor.
#[derive(Debug, Default)]
pub struct ExecutorStats {
    pub total_tasks: AtomicU64,
    pub parallel_groups: AtomicU64,
    pub sequential_fallbacks: AtomicU64,
    pub total_wall_time_us: AtomicU64,
    pub total_cpu_time_us: AtomicU64,
}

impl ExecutorStats {
    /// Snapshot current stats.
    pub fn snapshot(&self) -> ExecutorStatsSnapshot {
        ExecutorStatsSnapshot {
            total_tasks: self.total_tasks.load(Ordering::Relaxed),
            parallel_groups: self.parallel_groups.load(Ordering::Relaxed),
            sequential_fallbacks: self.sequential_fallbacks.load(Ordering::Relaxed),
            total_wall_time_us: self.total_wall_time_us.load(Ordering::Relaxed),
            total_cpu_time_us: self.total_cpu_time_us.load(Ordering::Relaxed),
        }
    }
}

/// Serializable snapshot of executor statistics.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ExecutorStatsSnapshot {
    pub total_tasks: u64,
    pub parallel_groups: u64,
    pub sequential_fallbacks: u64,
    pub total_wall_time_us: u64,
    pub total_cpu_time_us: u64,
}

/// Dependency-aware parallel executor for agent workflows.
///
/// Mirrors AlloyStack's group-based parallelism model:
/// - Tasks within the same group run concurrently.
/// - Groups execute sequentially (each group waits for completion before the next).
/// - A semaphore limits maximum concurrency.
pub struct ParallelWorkflowExecutor {
    max_concurrency: usize,
    semaphore: Arc<Semaphore>,
    stats: ExecutorStats,
}

impl ParallelWorkflowExecutor {
    /// Create a new executor with the given concurrency limit.
    pub fn new(max_concurrency: usize) -> Self {
        let max = max_concurrency.max(1);
        Self {
            max_concurrency: max,
            semaphore: Arc::new(Semaphore::new(max)),
            stats: ExecutorStats::default(),
        }
    }

    /// Maximum concurrency level.
    pub fn max_concurrency(&self) -> usize {
        self.max_concurrency
    }

    /// Execute a group of independent async tasks in parallel.
    ///
    /// All tasks in the group are launched concurrently (bounded by semaphore).
    /// Returns after all tasks complete.
    pub async fn execute_group<F, T>(&self, tasks: Vec<(String, F)>) -> GroupResult<T>
    where
        F: Future<Output = Result<T, anyhow::Error>> + Send + 'static,
        T: Send + 'static,
    {
        let task_count = tasks.len();
        if task_count == 0 {
            return GroupResult {
                results: Vec::new(),
                wall_time: Duration::ZERO,
                cpu_time: Duration::ZERO,
                parallelism_ratio: 1.0,
            };
        }

        self.stats
            .total_tasks
            .fetch_add(task_count as u64, Ordering::Relaxed);

        if task_count == 1 {
            self.stats
                .sequential_fallbacks
                .fetch_add(1, Ordering::Relaxed);
        } else {
            self.stats.parallel_groups.fetch_add(1, Ordering::Relaxed);
        }

        let group_start = Instant::now();
        let mut join_set = JoinSet::new();
        let sem = Arc::clone(&self.semaphore);

        for (id, future) in tasks {
            let sem = Arc::clone(&sem);
            join_set.spawn(async move {
                let permit = sem.acquire_owned().await.expect("semaphore closed");
                let task_start = Instant::now();
                let result = future.await;
                let task_duration = task_start.elapsed();
                drop(permit);
                (id, result, task_duration)
            });
        }

        let mut results = Vec::with_capacity(task_count);
        let mut total_cpu = Duration::ZERO;

        while let Some(join_result) = join_set.join_next().await {
            match join_result {
                Ok((id, result, duration)) => {
                    total_cpu += duration;
                    results.push((id, result));
                }
                Err(join_err) => {
                    results.push((
                        "<join_error>".to_owned(),
                        Err(anyhow::anyhow!("task join error: {}", join_err)),
                    ));
                }
            }
        }

        let wall_time = group_start.elapsed();
        let parallelism_ratio = if wall_time.as_nanos() > 0 {
            total_cpu.as_nanos() as f64 / wall_time.as_nanos() as f64
        } else {
            1.0
        };

        self.stats
            .total_wall_time_us
            .fetch_add(wall_time.as_micros() as u64, Ordering::Relaxed);
        self.stats
            .total_cpu_time_us
            .fetch_add(total_cpu.as_micros() as u64, Ordering::Relaxed);

        GroupResult {
            results,
            wall_time,
            cpu_time: total_cpu,
            parallelism_ratio,
        }
    }

    /// Execute a DAG of tasks respecting dependency order.
    ///
    /// Tasks are grouped into levels by topological sort:
    /// - Level 0: tasks with no dependencies.
    /// - Level N: tasks whose dependencies are all in levels < N.
    ///
    /// Each level is executed as a parallel group before proceeding to the next.
    /// This directly mirrors AlloyStack's sequential-groups-of-parallel-apps pattern.
    pub async fn execute_dag<F, Fut, T>(
        &self,
        items: Vec<WorkItem>,
        task_fn: F,
    ) -> Result<Vec<GroupResult<T>>, anyhow::Error>
    where
        F: Fn(String) -> Fut,
        Fut: Future<Output = Result<T, anyhow::Error>> + Send + 'static,
        T: Send + 'static,
    {
        let levels = topological_levels(&items)?;
        let mut group_results = Vec::with_capacity(levels.len());

        for level in levels {
            let tasks: Vec<(String, Fut)> = level
                .into_iter()
                .map(|id| {
                    let fut = task_fn(id.clone());
                    (id, fut)
                })
                .collect();

            let result = self.execute_group(tasks).await;
            group_results.push(result);
        }

        Ok(group_results)
    }

    /// Get stats snapshot.
    pub fn stats(&self) -> ExecutorStatsSnapshot {
        self.stats.snapshot()
    }
}

/// Topological sort into levels for parallel execution.
///
/// Returns groups of task IDs where each group's dependencies are satisfied
/// by all previous groups.
fn topological_levels(items: &[WorkItem]) -> Result<Vec<Vec<String>>, anyhow::Error> {
    let mut in_degree: HashMap<&str, usize> = HashMap::new();
    let mut dependents: HashMap<&str, Vec<&str>> = HashMap::new();
    let mut all_ids: HashSet<&str> = HashSet::new();

    for item in items {
        all_ids.insert(&item.id);
        in_degree.entry(&item.id).or_insert(0);
        for dep in &item.dependencies {
            dependents.entry(dep.as_str()).or_default().push(&item.id);
            *in_degree.entry(&item.id).or_insert(0) += 1;
        }
    }

    let mut levels = Vec::new();
    let mut queue: VecDeque<&str> = in_degree
        .iter()
        .filter(|(_, &deg)| deg == 0)
        .map(|(&id, _)| id)
        .collect();

    let mut processed = 0usize;

    while !queue.is_empty() {
        let current_level: Vec<String> = queue.drain(..).map(|s| s.to_owned()).collect();
        processed += current_level.len();

        let mut next_queue = VecDeque::new();
        for id in &current_level {
            if let Some(deps) = dependents.get(id.as_str()) {
                for &dep_id in deps {
                    let deg = in_degree.get_mut(dep_id).unwrap();
                    *deg -= 1;
                    if *deg == 0 {
                        next_queue.push_back(dep_id);
                    }
                }
            }
        }

        levels.push(current_level);
        queue = next_queue;
    }

    if processed != all_ids.len() {
        return Err(anyhow::anyhow!(
            "cyclic dependency detected in workflow DAG"
        ));
    }

    Ok(levels)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_topological_levels_linear() {
        let items = vec![
            WorkItem {
                id: "a".into(),
                dependencies: vec![],
            },
            WorkItem {
                id: "b".into(),
                dependencies: vec!["a".into()],
            },
            WorkItem {
                id: "c".into(),
                dependencies: vec!["b".into()],
            },
        ];

        let levels = topological_levels(&items).unwrap();
        assert_eq!(levels.len(), 3);
        assert_eq!(levels[0], vec!["a"]);
        assert_eq!(levels[1], vec!["b"]);
        assert_eq!(levels[2], vec!["c"]);
    }

    #[test]
    fn test_topological_levels_parallel() {
        let items = vec![
            WorkItem {
                id: "a".into(),
                dependencies: vec![],
            },
            WorkItem {
                id: "b".into(),
                dependencies: vec![],
            },
            WorkItem {
                id: "c".into(),
                dependencies: vec!["a".into(), "b".into()],
            },
        ];

        let levels = topological_levels(&items).unwrap();
        assert_eq!(levels.len(), 2);
        assert_eq!(levels[0].len(), 2); // a and b in parallel
        assert_eq!(levels[1], vec!["c"]);
    }

    #[test]
    fn test_cyclic_dependency() {
        let items = vec![
            WorkItem {
                id: "a".into(),
                dependencies: vec!["b".into()],
            },
            WorkItem {
                id: "b".into(),
                dependencies: vec!["a".into()],
            },
        ];

        assert!(topological_levels(&items).is_err());
    }

    #[tokio::test]
    async fn test_execute_group_parallel() {
        let executor = ParallelWorkflowExecutor::new(4);

        let tasks: Vec<(String, _)> = (0..4)
            .map(|i| {
                let id = format!("task_{}", i);
                let fut = async move {
                    tokio::time::sleep(Duration::from_millis(10)).await;
                    Ok(i)
                };
                (id, fut)
            })
            .collect();

        let result = executor.execute_group(tasks).await;
        assert_eq!(result.results.len(), 4);
        // Wall time should be ~10ms (parallel), not ~40ms (sequential).
        assert!(result.wall_time < Duration::from_millis(30));
        // Parallelism ratio should be > 1.0.
        assert!(result.parallelism_ratio > 1.0);
    }

    #[tokio::test]
    async fn test_execute_empty_group() {
        let executor = ParallelWorkflowExecutor::new(4);
        let tasks: Vec<(String, std::pin::Pin<Box<dyn Future<Output = Result<(), anyhow::Error>> + Send>>)> = vec![];
        let result = executor.execute_group(tasks).await;
        assert!(result.results.is_empty());
    }

    #[tokio::test]
    async fn test_execute_dag() {
        let executor = ParallelWorkflowExecutor::new(4);

        let items = vec![
            WorkItem {
                id: "a".into(),
                dependencies: vec![],
            },
            WorkItem {
                id: "b".into(),
                dependencies: vec![],
            },
            WorkItem {
                id: "c".into(),
                dependencies: vec!["a".into(), "b".into()],
            },
        ];

        let results = executor
            .execute_dag(items, |id| async move { Ok(format!("done_{}", id)) })
            .await
            .unwrap();

        // 2 levels: [a, b] then [c]
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].results.len(), 2);
        assert_eq!(results[1].results.len(), 1);
    }
}
