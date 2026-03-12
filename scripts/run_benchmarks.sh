#!/bin/bash
# Performance Benchmark Runner for AgentOS Integration
# This script runs Zeroclaw benchmarks and collects results for analysis

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"
ZEROCLAW_DIR="$PROJECT_ROOT/zeroclaw"
RESULTS_DIR="$PROJECT_ROOT/benchmark_results"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

echo_info() {
    echo -e "${GREEN}[INFO]${NC} $1"
}

echo_warn() {
    echo -e "${YELLOW}[WARN]${NC} $1"
}

echo_error() {
    echo -e "${RED}[ERROR]${NC} $1"
}

# Create results directory
mkdir -p "$RESULTS_DIR"
TIMESTAMP=$(date +%Y%m%d_%H%M%S)
RUN_DIR="$RESULTS_DIR/run_$TIMESTAMP"
mkdir -p "$RUN_DIR"

echo_info "Starting Zeroclaw performance benchmarks..."
echo_info "Results will be saved to: $RUN_DIR"

# Check if cargo is available
if ! command -v cargo &> /dev/null; then
    echo_error "cargo not found. Please install Rust toolchain."
    exit 1
fi

cd "$ZEROCLAW_DIR"

# Function to run a specific benchmark
run_benchmark() {
    local bench_name=$1
    local output_file="$RUN_DIR/${bench_name}.txt"

    echo_info "Running benchmark: $bench_name"

    if cargo bench --bench "$bench_name" -- --noplot 2>&1 | tee "$output_file"; then
        echo_info "Benchmark $bench_name completed successfully"
    else
        echo_warn "Benchmark $bench_name had some issues (check $output_file)"
    fi
}

# Parse command line arguments
BENCH_TYPE="${1:-all}"

case "$BENCH_TYPE" in
    "agent")
        echo_info "Running agent benchmarks only..."
        run_benchmark "agent_benchmarks"
        ;;
    "bottleneck")
        echo_info "Running bottleneck benchmarks only..."
        run_benchmark "bottleneck_benchmarks"
        ;;
    "all")
        echo_info "Running all benchmarks..."
        run_benchmark "agent_benchmarks"
        run_benchmark "bottleneck_benchmarks"
        ;;
    *)
        echo_error "Unknown benchmark type: $BENCH_TYPE"
        echo "Usage: $0 [agent|bottleneck|all]"
        exit 1
        ;;
esac

# Generate summary report
SUMMARY_FILE="$RUN_DIR/summary.md"
echo_info "Generating summary report..."

cat > "$SUMMARY_FILE" << EOF
# Zeroclaw Performance Benchmark Results

**Run Time**: $(date)
**Run ID**: $TIMESTAMP

## System Information

- **OS**: $(uname -s)
- **Kernel**: $(uname -r)
- **CPU**: $(grep 'model name' /proc/cpuinfo 2>/dev/null | head -1 | cut -d: -f2 | xargs || echo "Unknown")
- **Memory**: $(free -h 2>/dev/null | grep Mem | awk '{print $2}' || echo "Unknown")

## Benchmark Results

### Key Metrics (extracted from Criterion output)

EOF

# Extract key metrics from benchmark results
for result_file in "$RUN_DIR"/*.txt; do
    if [[ -f "$result_file" ]]; then
        bench_name=$(basename "$result_file" .txt)
        echo "### $bench_name" >> "$SUMMARY_FILE"
        echo "" >> "$SUMMARY_FILE"
        echo '```' >> "$SUMMARY_FILE"
        grep -E "(time:|thrpt:|change:)" "$result_file" | head -50 >> "$SUMMARY_FILE" 2>/dev/null || echo "No timing data extracted"
        echo '```' >> "$SUMMARY_FILE"
        echo "" >> "$SUMMARY_FILE"
    fi
done

cat >> "$SUMMARY_FILE" << EOF

## Analysis Notes

### Memory Operation Bottlenecks

- **Store latency**: Check \`memory_store_by_size\` results
- **Recall latency**: Check \`memory_recall_by_dataset_size\` results
- **Concurrent access**: Check \`memory_concurrent\` results

### Tool Execution Bottlenecks

- **Dispatch overhead**: Check \`tool_execution_overhead\` results
- **Parallel vs Sequential**: Check \`tool_parallel_vs_sequential\` results

### IPC Overhead (for AlloyStack integration)

- **Serialization cost**: Check \`data_serialization\` results
- **Lock contention**: Check \`mutex_contention\` results

## Next Steps

1. Compare with AlloyStack baseline benchmarks
2. Identify components for optimization
3. Design integration adapters based on bottleneck analysis

EOF

echo_info "Summary report generated: $SUMMARY_FILE"
echo_info "Benchmark run completed!"
echo ""
echo "Results saved to: $RUN_DIR"
echo "View summary: cat $SUMMARY_FILE"
