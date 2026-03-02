#!/usr/bin/env python3
"""Parse criterion benchmark output and generate a markdown performance report.

Usage:
    python3 generate_report.py --criterion-dir target/criterion --output reports/performance_report.md
"""

import argparse
import json
import os
import sys
from pathlib import Path
from datetime import datetime


def parse_estimates(estimates_path: Path) -> dict:
    """Parse criterion estimates.json and return timing info."""
    try:
        with open(estimates_path) as f:
            data = json.load(f)
        mean = data.get("mean", {})
        median = data.get("median", {})
        std_dev = data.get("std_dev", {})
        return {
            "mean_ns": mean.get("point_estimate", 0),
            "median_ns": median.get("point_estimate", 0),
            "std_dev_ns": std_dev.get("point_estimate", 0),
            "mean_ms": mean.get("point_estimate", 0) / 1_000_000,
            "median_ms": median.get("point_estimate", 0) / 1_000_000,
            "std_dev_ms": std_dev.get("point_estimate", 0) / 1_000_000,
        }
    except (json.JSONDecodeError, FileNotFoundError, KeyError):
        return None


def find_benchmarks(criterion_dir: Path) -> dict:
    """Walk criterion output directory and collect all benchmark results."""
    benchmarks = {}
    if not criterion_dir.exists():
        return benchmarks

    for group_dir in sorted(criterion_dir.iterdir()):
        if not group_dir.is_dir() or group_dir.name == "report":
            continue

        group_name = group_dir.name

        # Check for direct estimates (simple benchmarks).
        est_path = group_dir / "new" / "estimates.json"
        if est_path.exists():
            data = parse_estimates(est_path)
            if data:
                benchmarks[group_name] = data
            continue

        # Check for parameterized benchmarks (subdirectories).
        for bench_dir in sorted(group_dir.iterdir()):
            if not bench_dir.is_dir():
                continue
            est_path = bench_dir / "new" / "estimates.json"
            if est_path.exists():
                data = parse_estimates(est_path)
                if data:
                    full_name = f"{group_name}/{bench_dir.name}"
                    benchmarks[full_name] = data

    return benchmarks


def compute_speedups(benchmarks: dict) -> list:
    """Find baseline/alloyfang pairs and compute speedup ratios."""
    pairs = []

    # Group by benchmark group.
    groups = {}
    for name, data in benchmarks.items():
        parts = name.split("/")
        if len(parts) >= 2:
            group = parts[0]
            variant = "/".join(parts[1:])
        else:
            group = name
            variant = name

        if group not in groups:
            groups[group] = {}
        groups[group][variant] = data

    # Find pairs.
    for group, variants in sorted(groups.items()):
        baseline_key = None
        optimized_key = None

        for key in variants:
            key_lower = key.lower()
            if any(tag in key_lower for tag in ["baseline", "eager", "sequential", "json", "clone"]):
                baseline_key = key
            elif any(tag in key_lower for tag in ["alloyfang", "lazy", "parallel", "shared_buffer", "buffer_pool"]):
                optimized_key = key

        if baseline_key and optimized_key:
            b = variants[baseline_key]
            o = variants[optimized_key]
            speedup = b["mean_ns"] / o["mean_ns"] if o["mean_ns"] > 0 else 0
            pairs.append({
                "group": group,
                "baseline_name": baseline_key,
                "optimized_name": optimized_key,
                "baseline_ms": b["mean_ms"],
                "optimized_ms": o["mean_ms"],
                "speedup": speedup,
            })

    return pairs


def generate_report(benchmarks: dict, pairs: list, output_path: Path):
    """Generate markdown performance report."""
    now = datetime.now().strftime("%Y-%m-%d %H:%M:%S")

    lines = []
    lines.append("# AloyFang Performance Report")
    lines.append("")
    lines.append(f"Generated: {now}")
    lines.append("")
    lines.append("## Architecture")
    lines.append("")
    lines.append("- **Baseline**: OpenFang standalone (all AlloyStack optimizations disabled)")
    lines.append("- **AloyFang**: OpenFang + AlloyStack optimizations (lazy loading, reference passing, parallel execution)")
    lines.append("- **AlloyStack paper**: EuroSys 2025 — on-demand loading, zero-copy reference passing, parallel group execution")
    lines.append("")

    # Summary table.
    if pairs:
        lines.append("## Summary: Optimization Speedups")
        lines.append("")
        lines.append("| Benchmark Group | Baseline | AloyFang | Speedup |")
        lines.append("|----------------|----------|----------|---------|")
        for p in pairs:
            b_str = format_time(p["baseline_ms"])
            o_str = format_time(p["optimized_ms"])
            lines.append(f"| {p['group']} | {b_str} | {o_str} | **{p['speedup']:.1f}x** |")
        lines.append("")

        # Highlight best speedups.
        best = max(pairs, key=lambda x: x["speedup"])
        lines.append(f"**Best speedup**: {best['group']} — **{best['speedup']:.1f}x** faster with AloyFang")
        lines.append("")

    # All benchmarks.
    lines.append("## Detailed Results")
    lines.append("")
    lines.append("| Benchmark | Mean | Median | Std Dev |")
    lines.append("|-----------|------|--------|---------|")
    for name, data in sorted(benchmarks.items()):
        lines.append(
            f"| {name} | {format_time(data['mean_ms'])} | "
            f"{format_time(data['median_ms'])} | {format_time(data['std_dev_ms'])} |"
        )
    lines.append("")

    # Optimization analysis.
    lines.append("## Optimization Analysis")
    lines.append("")
    lines.append("### 1. On-Demand Loading (Lazy Loader)")
    lines.append("")
    lines.append("Inspired by AlloyStack's `ServiceLoader::service_or_load()` which loads")
    lines.append("dynamic libraries via `dlmopen` only when first referenced.")
    lines.append("AloyFang adapts this by lazily compiling WASM modules with LRU caching.")
    lines.append("")

    cold_start_pairs = [p for p in pairs if "cold_start" in p["group"].lower()]
    if cold_start_pairs:
        for p in cold_start_pairs:
            lines.append(f"- {p['group']}: {p['speedup']:.1f}x improvement")
    lines.append("")

    lines.append("### 2. Reference Passing (Zero-Copy Shared Buffers)")
    lines.append("")
    lines.append("Inspired by AlloyStack's `faas_buffer` which uses named buffer slots")
    lines.append("for zero-copy data transfer between serverless functions.")
    lines.append("AloyFang adapts this with `DashMap<String, Bytes>` for O(1) cloning.")
    lines.append("")

    ref_pairs = [p for p in pairs if "data_transfer" in p["group"].lower() or "multi_slot" in p["group"].lower()]
    if ref_pairs:
        for p in ref_pairs:
            lines.append(f"- {p['group']}: {p['speedup']:.1f}x improvement")
    lines.append("")

    lines.append("### 3. Parallel Execution (Workflow Executor)")
    lines.append("")
    lines.append("Inspired by AlloyStack's `run_group_in_parallel()` which uses")
    lines.append("`thread::scope` for concurrent function execution within isolation groups.")
    lines.append("AloyFang adapts this with `tokio::JoinSet` + `Semaphore` for async parallelism.")
    lines.append("")

    par_pairs = [p for p in pairs if "parallel" in p["group"].lower() or "dag" in p["group"].lower()]
    if par_pairs:
        for p in par_pairs:
            lines.append(f"- {p['group']}: {p['speedup']:.1f}x improvement")
    lines.append("")

    # Footer.
    lines.append("---")
    lines.append("")
    lines.append("*Report generated by AloyFang benchmark suite.*")
    lines.append(f"*Criterion HTML report: `target/criterion/report/index.html`*")
    lines.append("")

    output_path.parent.mkdir(parents=True, exist_ok=True)
    with open(output_path, "w") as f:
        f.write("\n".join(lines))

    print(f"Report written to {output_path}")
    print(f"  Total benchmarks: {len(benchmarks)}")
    print(f"  Comparison pairs: {len(pairs)}")
    if pairs:
        avg_speedup = sum(p["speedup"] for p in pairs) / len(pairs)
        print(f"  Average speedup: {avg_speedup:.1f}x")


def format_time(ms: float) -> str:
    """Format milliseconds as human-readable string."""
    if ms < 0.001:
        return f"{ms * 1_000_000:.0f}ns"
    elif ms < 1:
        return f"{ms * 1_000:.1f}us"
    elif ms < 1000:
        return f"{ms:.2f}ms"
    else:
        return f"{ms / 1000:.2f}s"


def main():
    parser = argparse.ArgumentParser(description="Generate AloyFang performance report")
    parser.add_argument(
        "--criterion-dir",
        type=Path,
        default=Path("target/criterion"),
        help="Path to criterion output directory",
    )
    parser.add_argument(
        "--output",
        type=Path,
        default=Path("reports/performance_report.md"),
        help="Output markdown report path",
    )
    args = parser.parse_args()

    print(f"Scanning {args.criterion_dir}...")
    benchmarks = find_benchmarks(args.criterion_dir)

    if not benchmarks:
        print("No benchmark results found. Run 'cargo bench' first.")
        sys.exit(1)

    print(f"Found {len(benchmarks)} benchmark results")

    pairs = compute_speedups(benchmarks)
    print(f"Found {len(pairs)} baseline/optimized comparison pairs")

    generate_report(benchmarks, pairs, args.output)


if __name__ == "__main__":
    main()
