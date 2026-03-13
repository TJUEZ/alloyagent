//! AlloyStack optimization layer for OpenFang.
//!
//! This module integrates AlloyStack's key optimizations into OpenFang:
//! - On-demand loading: Lazy loading of WASM modules to reduce cold start latency
//! - Reference passing: Zero-copy data transfer between agents
//! - Parallel execution: Concurrent workflow execution with dependency tracking
//!
//! Based on the AlloyStack paper (EuroSys 2025):
//! "AlloyStack: A Library Operating System for Serverless Workflow Applications"

pub mod lazy_loader;
pub mod reference_passing;
pub mod parallel_executor;
pub mod metrics;

pub use lazy_loader::LazyWasmLoader;
pub use reference_passing::SharedBuffer;
pub use parallel_executor::ParallelWorkflowExecutor;
pub use metrics::OptimMetrics;

use std::sync::Arc;
use tokio::sync::RwLock;

/// AlloyStack optimization configuration
#[derive(Debug, Clone)]
pub struct AlloyStackConfig {
    /// Enable on-demand loading for WASM modules
    pub enable_lazy_loading: bool,
    /// Enable reference passing for inter-agent data transfer
    pub enable_reference_passing: bool,
    /// Enable parallel workflow execution
    pub enable_parallel_execution: bool,
    /// Maximum number of cached WASM modules
    pub max_cached_modules: usize,
    /// Maximum shared buffer size in bytes
    pub max_shared_buffer_size: usize,
    /// Maximum parallel execution threads
    pub max_parallel_threads: usize,
}

impl Default for AlloyStackConfig {
    fn default() -> Self {
        Self {
            enable_lazy_loading: true,
            enable_reference_passing: true,
            enable_parallel_execution: true,
            max_cached_modules: 64,
            max_shared_buffer_size: 256 * 1024 * 1024, // 256MB
            max_parallel_threads: num_cpus::get(),
        }
    }
}

/// AlloyStack optimization runtime
pub struct AlloyStackRuntime {
    config: AlloyStackConfig,
    loader: Arc<RwLock<LazyWasmLoader>>,
    shared_buffers: Arc<RwLock<reference_passing::SharedBufferPool>>,
    executor: Arc<ParallelWorkflowExecutor>,
    metrics: Arc<RwLock<OptimMetrics>>,
}

impl AlloyStackRuntime {
    /// Create a new AlloyStack optimization runtime
    pub fn new(config: AlloyStackConfig) -> Self {
        let loader = Arc::new(RwLock::new(LazyWasmLoader::new(config.max_cached_modules)));
        let shared_buffers = Arc::new(RwLock::new(
            reference_passing::SharedBufferPool::new(config.max_shared_buffer_size),
        ));
        let executor = Arc::new(ParallelWorkflowExecutor::new(config.max_parallel_threads));
        let metrics = Arc::new(RwLock::new(OptimMetrics::new()));

        Self {
            config,
            loader,
            shared_buffers,
            executor,
            metrics,
        }
    }

    /// Get the lazy loader
    pub fn loader(&self) -> Arc<RwLock<LazyWasmLoader>> {
        Arc::clone(&self.loader)
    }

    /// Get the shared buffer pool
    pub fn shared_buffers(&self) -> Arc<RwLock<reference_passing::SharedBufferPool>> {
        Arc::clone(&self.shared_buffers)
    }

    /// Get the parallel executor
    pub fn executor(&self) -> Arc<ParallelWorkflowExecutor> {
        Arc::clone(&self.executor)
    }

    /// Get optimization metrics
    pub fn metrics(&self) -> Arc<RwLock<OptimMetrics>> {
        Arc::clone(&self.metrics)
    }

    /// Check if lazy loading is enabled
    pub fn lazy_loading_enabled(&self) -> bool {
        self.config.enable_lazy_loading
    }

    /// Check if reference passing is enabled
    pub fn reference_passing_enabled(&self) -> bool {
        self.config.enable_reference_passing
    }

    /// Check if parallel execution is enabled
    pub fn parallel_execution_enabled(&self) -> bool {
        self.config.enable_parallel_execution
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = AlloyStackConfig::default();
        assert!(config.enable_lazy_loading);
        assert!(config.enable_reference_passing);
        assert!(config.enable_parallel_execution);
        assert_eq!(config.max_cached_modules, 64);
    }

    #[test]
    fn test_runtime_creation() {
        let config = AlloyStackConfig::default();
        let runtime = AlloyStackRuntime::new(config);
        assert!(runtime.lazy_loading_enabled());
        assert!(runtime.reference_passing_enabled());
        assert!(runtime.parallel_execution_enabled());
    }
}
