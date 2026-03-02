//! AloyFang: OpenFang with AlloyStack optimizations.
//!
//! This crate provides benchmark infrastructure for comparing standalone OpenFang
//! performance against the AlloyStack-optimized version (AloyFang).

pub mod config;
pub mod mock_llm;
pub mod mock_tools;

pub use openfang_runtime::alloystack_optim::{
    AlloyStackConfig, AlloyStackRuntime, LazyWasmLoader, OptimMetrics, ParallelWorkflowExecutor,
    SharedBuffer,
};

use openfang_runtime::alloystack_optim::reference_passing::SharedBufferPool;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

/// AloyFang runtime — wraps AlloyStackRuntime with benchmark helpers.
pub struct AloyFangRuntime {
    inner: AlloyStackRuntime,
    created_at: Instant,
}

impl AloyFangRuntime {
    /// Create with all optimizations enabled (AloyFang mode).
    pub fn optimized() -> Self {
        let config = AlloyStackConfig::default();
        Self {
            inner: AlloyStackRuntime::new(config),
            created_at: Instant::now(),
        }
    }

    /// Create with all optimizations disabled (baseline mode).
    pub fn baseline() -> Self {
        let config = AlloyStackConfig {
            enable_lazy_loading: false,
            enable_reference_passing: false,
            enable_parallel_execution: false,
            ..AlloyStackConfig::default()
        };
        Self {
            inner: AlloyStackRuntime::new(config),
            created_at: Instant::now(),
        }
    }

    /// Get the underlying runtime.
    pub fn runtime(&self) -> &AlloyStackRuntime {
        &self.inner
    }

    /// Get shared buffer pool.
    pub fn shared_buffers(&self) -> Arc<RwLock<SharedBufferPool>> {
        self.inner.shared_buffers()
    }

    /// Get parallel executor.
    pub fn executor(&self) -> Arc<ParallelWorkflowExecutor> {
        self.inner.executor()
    }

    /// Get metrics.
    pub fn metrics(&self) -> Arc<RwLock<OptimMetrics>> {
        self.inner.metrics()
    }

    /// Time since creation.
    pub fn uptime(&self) -> Duration {
        self.created_at.elapsed()
    }
}

/// Utility: read current process RSS in KB (Linux).
pub fn get_rss_kb() -> usize {
    #[cfg(target_os = "linux")]
    {
        if let Ok(status) = std::fs::read_to_string("/proc/self/status") {
            for line in status.lines() {
                if line.starts_with("VmRSS:") {
                    let parts: Vec<&str> = line.split_whitespace().collect();
                    if parts.len() >= 2 {
                        return parts[1].parse().unwrap_or(0);
                    }
                }
            }
        }
        0
    }
    #[cfg(not(target_os = "linux"))]
    {
        0
    }
}
