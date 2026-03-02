//! Mock tool implementations for benchmarking.
//!
//! Provides tools that simulate various workload patterns without external
//! dependencies (no actual shell execution, network calls, etc.).

use std::time::Duration;

/// Result of a mock tool execution.
#[derive(Debug, Clone)]
pub struct MockToolResult {
    pub output: String,
    pub success: bool,
    pub duration: Duration,
}

/// A no-op tool that returns immediately (measures dispatch overhead).
pub fn noop_tool() -> MockToolResult {
    MockToolResult {
        output: "ok".to_owned(),
        success: true,
        duration: Duration::ZERO,
    }
}

/// A tool that sleeps for a configurable duration (simulates I/O-bound work).
pub async fn sleep_tool(duration: Duration) -> MockToolResult {
    tokio::time::sleep(duration).await;
    MockToolResult {
        output: "slept".to_owned(),
        success: true,
        duration,
    }
}

/// A tool that produces a large output (tests reference passing benefit).
pub fn large_output_tool(output_size: usize) -> MockToolResult {
    let data = "x".repeat(output_size);
    MockToolResult {
        output: data,
        success: true,
        duration: Duration::ZERO,
    }
}

/// A CPU-bound tool (tests parallel execution benefit).
pub fn cpu_bound_tool(iterations: usize) -> MockToolResult {
    let start = std::time::Instant::now();
    let mut sum = 0u64;
    for i in 0..iterations {
        sum = sum.wrapping_add(i as u64);
        // Prevent optimizer from eliding the loop.
        std::hint::black_box(sum);
    }
    let duration = start.elapsed();
    MockToolResult {
        output: format!("computed: {}", sum),
        success: true,
        duration,
    }
}

/// Simulate a tool chain: each tool produces output consumed by the next.
pub fn tool_chain(steps: usize, data_size: usize) -> Vec<MockToolResult> {
    let mut results = Vec::with_capacity(steps);
    let mut data = vec![0u8; data_size];
    for i in 0..steps {
        // Simulate processing: modify data in-place.
        for byte in data.iter_mut() {
            *byte = byte.wrapping_add(i as u8);
        }
        results.push(MockToolResult {
            output: format!("step_{}_output_{}_bytes", i, data.len()),
            success: true,
            duration: Duration::ZERO,
        });
    }
    results
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_noop_tool() {
        let result = noop_tool();
        assert!(result.success);
        assert_eq!(result.duration, Duration::ZERO);
    }

    #[tokio::test]
    async fn test_sleep_tool() {
        let result = sleep_tool(Duration::from_millis(5)).await;
        assert!(result.success);
    }

    #[test]
    fn test_large_output() {
        let result = large_output_tool(1000);
        assert_eq!(result.output.len(), 1000);
    }

    #[test]
    fn test_cpu_bound() {
        let result = cpu_bound_tool(100_000);
        assert!(result.success);
        assert!(result.duration > Duration::ZERO);
    }

    #[test]
    fn test_tool_chain() {
        let results = tool_chain(5, 1024);
        assert_eq!(results.len(), 5);
    }
}
