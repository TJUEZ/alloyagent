# alloyfang

AloyFang: OpenFang Agent OS with AlloyStack serverless optimizations — benchmark suite.

## Overview

This crate provides benchmark infrastructure for comparing standalone OpenFang performance against the AlloyStack-optimized version (AloyFang).

## Benchmarks

- **cold_start**: Cold start time measurement
- **memory_usage**: Memory consumption analysis
- **tool_execution**: Tool execution performance
- **reference_passing**: Reference passing optimization
- **parallel_execution**: Parallel workflow execution
- **agent_loop**: Agent loop performance

## Building

```bash
# Build the benchmark suite
cargo build --release -p alloyfang
```

## Running Benchmarks

```bash
# Run all benchmarks
./scripts/run_all_benchmarks.sh

# Run with criterion (generates HTML reports)
cargo bench
```

## Architecture

The benchmark suite tests three key AlloyStack optimizations:

1. **Lazy Loading**: On-demand WASM module loading
2. **Reference Passing**: Zero-copy data sharing between functions
3. **Parallel Execution**: Concurrent workflow execution

## Dependencies

- [OpenFang](https://github.com/TJUEZ/openfang): Agent Operating System
- [AlloyStack](https://github.com/TJUEZ/alloystack): Library OS for serverless workflows
