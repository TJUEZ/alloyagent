# AlloyStack Benchmarks

Benchmark suite for AlloyStack serverless workflow optimizations.

## Projects

| Project | Description |
|--------|-------------|
| **alloyfang** | Benchmarks comparing OpenFang vs AlloyStack-optimized performance |

## Quick Start

```bash
# Build benchmarks
cargo build --release -p alloyfang

# Run all benchmarks
./scripts/run_all_benchmarks.sh

# Run with criterion (generates HTML reports)
cargo bench -p alloyfang
```

## Benchmark Suite

The `alloyfang` crate provides benchmarks for three key AlloyStack optimizations:

1. **Lazy Loading**: On-demand WASM module loading
2. **Reference Passing**: Zero-copy data sharing between functions
3. **Parallel Execution**: Concurrent workflow execution

### Available Benchmarks

- `cold_start`: Cold start time measurement
- `memory_usage`: Memory consumption analysis
- `tool_execution`: Tool execution performance
- `reference_passing`: Reference passing optimization
- `parallel_execution`: Parallel workflow execution
- `agent_loop`: Agent loop performance

## Requirements

- Rust 2021 edition
- Linux (for memory measurements)
