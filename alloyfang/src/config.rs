//! AloyFang benchmark configuration.

/// Configuration for benchmark runs.
#[derive(Debug, Clone)]
pub struct BenchConfig {
    /// Number of WASM modules to register in cold start tests.
    pub num_modules: usize,
    /// Data sizes to test in reference passing benchmarks (bytes).
    pub data_sizes: Vec<usize>,
    /// Number of parallel tasks to test.
    pub parallel_task_counts: Vec<usize>,
    /// Simulated LLM latency for mock driver.
    pub mock_llm_latency_ms: u64,
    /// Simulated tool execution latency.
    pub mock_tool_latency_ms: u64,
}

impl Default for BenchConfig {
    fn default() -> Self {
        Self {
            num_modules: 10,
            data_sizes: vec![1_000, 10_000, 100_000, 1_000_000, 10_000_000],
            parallel_task_counts: vec![2, 4, 8, 16],
            mock_llm_latency_ms: 50,
            mock_tool_latency_ms: 10,
        }
    }
}
