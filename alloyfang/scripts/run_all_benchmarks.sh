#!/bin/bash
set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
REPORT_DIR="$PROJECT_DIR/reports"

echo "=== AloyFang Benchmark Suite ==="
echo "Project: $PROJECT_DIR"
echo "Reports: $REPORT_DIR"
echo ""

cd "$PROJECT_DIR"

# Build first to separate compile time from bench time.
echo "[1/4] Building benchmarks..."
cargo bench --no-run 2>&1 | tail -5

# Run all benchmarks.
echo ""
echo "[2/4] Running benchmarks..."
cargo bench 2>&1 | tee "$REPORT_DIR/benchmark_raw.txt"

# Generate markdown report.
echo ""
echo "[3/4] Generating performance report..."
python3 "$SCRIPT_DIR/generate_report.py" \
    --criterion-dir "$PROJECT_DIR/target/criterion" \
    --output "$REPORT_DIR/performance_report.md"

# Summary.
echo ""
echo "[4/4] Done!"
echo "  Raw output: $REPORT_DIR/benchmark_raw.txt"
echo "  Report:     $REPORT_DIR/performance_report.md"
echo "  HTML:       $PROJECT_DIR/target/criterion/report/index.html"
