//! Performance metrics collection for AlloyStack optimizations.
//!
//! Inspired by AlloyStack's `MetricBucket` / `SvcMetricBucket` pattern
//! in `libasvisor/src/metric.rs`, which tracks isolation begin/end timestamps,
//! per-service initialization and run times, and memory consumption over time.

use std::time::{Duration, Instant};

/// A snapshot of memory usage at a point in time.
#[derive(Debug, Clone, serde::Serialize)]
pub struct MemorySample {
    /// Microseconds since metrics collection started.
    pub offset_us: u128,
    /// Resident set size in kilobytes (from /proc/self/status VmRSS).
    pub rss_kb: usize,
}

/// Per-component timing record.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ComponentTiming {
    pub name: String,
    pub init_duration: Duration,
    pub run_duration: Duration,
}

/// Comprehensive optimization metrics.
///
/// Mirrors AlloyStack's two-tier metric collection:
/// - Isolation-level: total duration, memory samples
/// - Service-level: per-component init and run times
#[derive(Debug, Clone, serde::Serialize)]
pub struct OptimMetrics {
    // === Timing ===
    /// Time from runtime creation to first task readiness.
    pub cold_start_time: Option<Duration>,
    /// Time for subsequent starts (cached).
    pub warm_start_time: Option<Duration>,

    // === Lazy Loader ===
    pub wasm_cache_hits: u64,
    pub wasm_cache_misses: u64,
    #[serde(with = "duration_ms")]
    pub wasm_compile_time_total: Duration,

    // === Reference Passing ===
    pub buffer_allocations: u64,
    pub zero_copy_transfers: u64,
    pub bytes_saved_by_reference: u64,
    pub buffer_evictions: u64,

    // === Parallel Execution ===
    pub parallel_groups_executed: u64,
    pub tasks_parallelized: u64,
    pub parallelism_speedup_samples: Vec<f64>,

    // === Memory Tracking ===
    pub memory_samples: Vec<MemorySample>,

    // === Component Timings ===
    pub component_timings: Vec<ComponentTiming>,

    // === Agent Loop ===
    pub tool_execution_times: Vec<(String, Duration)>,
    pub llm_call_times: Vec<Duration>,
    pub loop_iterations: u32,

    // === Internal ===
    #[serde(skip)]
    started_at: Option<Instant>,
}

/// Custom serde for Duration as milliseconds.
mod duration_ms {
    use serde::Serializer;
    use std::time::Duration;
    pub fn serialize<S: Serializer>(d: &Duration, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_f64(d.as_secs_f64() * 1000.0)
    }
}

impl OptimMetrics {
    /// Create a new empty metrics collection.
    pub fn new() -> Self {
        Self {
            cold_start_time: None,
            warm_start_time: None,
            wasm_cache_hits: 0,
            wasm_cache_misses: 0,
            wasm_compile_time_total: Duration::ZERO,
            buffer_allocations: 0,
            zero_copy_transfers: 0,
            bytes_saved_by_reference: 0,
            buffer_evictions: 0,
            parallel_groups_executed: 0,
            tasks_parallelized: 0,
            parallelism_speedup_samples: Vec::new(),
            memory_samples: Vec::new(),
            component_timings: Vec::new(),
            tool_execution_times: Vec::new(),
            llm_call_times: Vec::new(),
            loop_iterations: 0,
            started_at: Some(Instant::now()),
        }
    }

    /// Record cold start duration.
    pub fn record_cold_start(&mut self, duration: Duration) {
        self.cold_start_time = Some(duration);
    }

    /// Record warm start duration.
    pub fn record_warm_start(&mut self, duration: Duration) {
        self.warm_start_time = Some(duration);
    }

    /// Record a tool execution timing.
    pub fn record_tool_execution(&mut self, tool_name: &str, duration: Duration) {
        self.tool_execution_times
            .push((tool_name.to_owned(), duration));
    }

    /// Record an LLM call timing.
    pub fn record_llm_call(&mut self, duration: Duration) {
        self.llm_call_times.push(duration);
    }

    /// Record a component init + run timing.
    pub fn record_component_timing(
        &mut self,
        name: &str,
        init_duration: Duration,
        run_duration: Duration,
    ) {
        self.component_timings.push(ComponentTiming {
            name: name.to_owned(),
            init_duration,
            run_duration,
        });
    }

    /// Record a parallelism speedup sample.
    pub fn record_parallelism_speedup(&mut self, ratio: f64) {
        self.parallelism_speedup_samples.push(ratio);
        self.parallel_groups_executed += 1;
    }

    /// Increment loop iteration counter.
    pub fn increment_loop_iterations(&mut self) {
        self.loop_iterations += 1;
    }

    /// Sample current memory usage from /proc/self/status.
    pub fn sample_memory(&mut self) {
        let rss_kb = get_current_vm_rss_kb();
        let offset = self
            .started_at
            .map(|s| s.elapsed().as_micros())
            .unwrap_or(0);
        self.memory_samples.push(MemorySample { offset_us: offset, rss_kb });
    }

    /// Update from lazy loader stats.
    pub fn update_from_loader(&mut self, hits: u64, misses: u64, compile_time: Duration) {
        self.wasm_cache_hits = hits;
        self.wasm_cache_misses = misses;
        self.wasm_compile_time_total = compile_time;
    }

    /// Update from buffer pool stats.
    pub fn update_from_buffers(
        &mut self,
        allocations: u64,
        zero_copy: u64,
        bytes_saved: u64,
        evictions: u64,
    ) {
        self.buffer_allocations = allocations;
        self.zero_copy_transfers = zero_copy;
        self.bytes_saved_by_reference = bytes_saved;
        self.buffer_evictions = evictions;
    }

    /// Export as JSON value.
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap_or_default()
    }

    /// Generate a human-readable markdown report.
    pub fn to_report(&self, label: &str) -> String {
        let mut report = String::new();
        report.push_str(&format!("## {} Performance Metrics\n\n", label));

        // Timing section
        report.push_str("### Timing\n\n");
        report.push_str("| Metric | Value |\n|--------|-------|\n");
        if let Some(cs) = self.cold_start_time {
            report.push_str(&format!("| Cold start | {:.2}ms |\n", cs.as_secs_f64() * 1000.0));
        }
        if let Some(ws) = self.warm_start_time {
            report.push_str(&format!("| Warm start | {:.2}ms |\n", ws.as_secs_f64() * 1000.0));
        }

        // Lazy loader section
        report.push_str("\n### WASM Lazy Loader\n\n");
        report.push_str("| Metric | Value |\n|--------|-------|\n");
        report.push_str(&format!("| Cache hits | {} |\n", self.wasm_cache_hits));
        report.push_str(&format!("| Cache misses | {} |\n", self.wasm_cache_misses));
        let hit_rate = if self.wasm_cache_hits + self.wasm_cache_misses > 0 {
            self.wasm_cache_hits as f64
                / (self.wasm_cache_hits + self.wasm_cache_misses) as f64
                * 100.0
        } else {
            0.0
        };
        report.push_str(&format!("| Hit rate | {:.1}% |\n", hit_rate));
        report.push_str(&format!(
            "| Total compile time | {:.2}ms |\n",
            self.wasm_compile_time_total.as_secs_f64() * 1000.0
        ));

        // Reference passing section
        report.push_str("\n### Reference Passing\n\n");
        report.push_str("| Metric | Value |\n|--------|-------|\n");
        report.push_str(&format!("| Buffer allocations | {} |\n", self.buffer_allocations));
        report.push_str(&format!("| Zero-copy transfers | {} |\n", self.zero_copy_transfers));
        report.push_str(&format!(
            "| Bytes saved | {} |\n",
            format_bytes(self.bytes_saved_by_reference)
        ));
        report.push_str(&format!("| Evictions | {} |\n", self.buffer_evictions));

        // Parallel execution section
        report.push_str("\n### Parallel Execution\n\n");
        report.push_str("| Metric | Value |\n|--------|-------|\n");
        report.push_str(&format!(
            "| Groups executed | {} |\n",
            self.parallel_groups_executed
        ));
        report.push_str(&format!("| Tasks parallelized | {} |\n", self.tasks_parallelized));
        if !self.parallelism_speedup_samples.is_empty() {
            let avg: f64 =
                self.parallelism_speedup_samples.iter().sum::<f64>()
                    / self.parallelism_speedup_samples.len() as f64;
            report.push_str(&format!("| Avg parallelism ratio | {:.2}x |\n", avg));
        }

        // Memory section
        if !self.memory_samples.is_empty() {
            report.push_str("\n### Memory Usage\n\n");
            report.push_str("| Time (ms) | RSS (KB) |\n|-----------|----------|\n");
            for sample in &self.memory_samples {
                report.push_str(&format!(
                    "| {:.1} | {} |\n",
                    sample.offset_us as f64 / 1000.0,
                    sample.rss_kb
                ));
            }
        }

        // Agent loop section
        report.push_str(&format!("\n### Agent Loop\n\n"));
        report.push_str(&format!("- Iterations: {}\n", self.loop_iterations));
        report.push_str(&format!("- LLM calls: {}\n", self.llm_call_times.len()));
        report.push_str(&format!(
            "- Tool executions: {}\n",
            self.tool_execution_times.len()
        ));

        if !self.llm_call_times.is_empty() {
            let avg_llm: f64 = self.llm_call_times.iter().map(|d| d.as_secs_f64() * 1000.0).sum::<f64>()
                / self.llm_call_times.len() as f64;
            report.push_str(&format!("- Avg LLM call: {:.2}ms\n", avg_llm));
        }

        if !self.tool_execution_times.is_empty() {
            let avg_tool: f64 = self
                .tool_execution_times
                .iter()
                .map(|(_, d)| d.as_secs_f64() * 1000.0)
                .sum::<f64>()
                / self.tool_execution_times.len() as f64;
            report.push_str(&format!("- Avg tool execution: {:.2}ms\n", avg_tool));
        }

        report
    }
}

impl Default for OptimMetrics {
    fn default() -> Self {
        Self::new()
    }
}

/// Read VmRSS from /proc/self/status (Linux only).
fn get_current_vm_rss_kb() -> usize {
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

/// Format bytes as human-readable string.
fn format_bytes(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.1} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_metrics() {
        let m = OptimMetrics::new();
        assert!(m.cold_start_time.is_none());
        assert_eq!(m.loop_iterations, 0);
        assert!(m.memory_samples.is_empty());
    }

    #[test]
    fn test_record_and_report() {
        let mut m = OptimMetrics::new();
        m.record_cold_start(Duration::from_millis(180));
        m.record_warm_start(Duration::from_millis(35));
        m.wasm_cache_hits = 10;
        m.wasm_cache_misses = 2;
        m.buffer_allocations = 50;
        m.zero_copy_transfers = 45;
        m.bytes_saved_by_reference = 1024 * 1024; // 1MB
        m.record_tool_execution("shell", Duration::from_millis(15));
        m.record_llm_call(Duration::from_millis(200));
        m.loop_iterations = 3;

        let report = m.to_report("AloyFang");
        assert!(report.contains("AloyFang"));
        assert!(report.contains("180.00ms"));
        assert!(report.contains("35.00ms"));
        assert!(report.contains("83.3%")); // hit rate
        assert!(report.contains("1.0 MB"));
    }

    #[test]
    fn test_to_json() {
        let mut m = OptimMetrics::new();
        m.record_cold_start(Duration::from_millis(100));
        let json = m.to_json();
        assert!(json.is_object());
        assert!(json.get("cold_start_time").is_some());
    }

    #[test]
    fn test_sample_memory() {
        let mut m = OptimMetrics::new();
        m.sample_memory();
        // On Linux, this should capture VmRSS; on other platforms, it's 0.
        assert_eq!(m.memory_samples.len(), 1);
    }

    #[test]
    fn test_format_bytes() {
        assert_eq!(format_bytes(500), "500 B");
        assert_eq!(format_bytes(1536), "1.5 KB");
        assert_eq!(format_bytes(1048576), "1.0 MB");
    }
}
