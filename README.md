# AlloyAgent

集成 AlloyStack 优化的 Agent 操作系统性能对比测试框架。

## 项目简介

AlloyAgent 是一个用于对比 **OpenFang**（原始版本）与 **AlloyFang**（AlloyStack 优化版本）性能的基准测试框架。

基于 [AlloyStack 论文 (EuroSys 2025)](https://github.com/alloystack/alloystack) 的核心技术，实现并验证了以下优化：

| 优化技术 | 描述 |
|---------|------|
| **Lazy Loading** | WASM 模块按需编译，首次访问时才加载，减少冷启动时间 |
| **Reference Passing** | 零拷贝数据传输，agent 间共享内存，减少序列化开销 |
| **Parallel Execution** | 并发工作流执行，带依赖跟踪，提升吞吐量 |

## 目录结构

```
alloyagent/
├── alloyfang/          # Benchmark 测试套件
│   ├── src/           # 运行时封装
│   └── benches/       # 性能测试用例
│
├── openfang/          # OpenFang Agent OS
│   └── crates/openfang-runtime/src/alloystack_optim/  # 优化实现
│       ├── lazy_loader.rs
│       ├── reference_passing.rs
│       ├── parallel_executor.rs
│       └── metrics.rs
│
├── AlloyStack/        # AlloyStack 参考实现
│
└── zeroclaw/          # ZeroClaw 机器人框架
```

## 快速开始

### 1. 构建项目

```bash
# 构建整个工作空间
cd /home/wzx/alloyagent/openfang
cargo build --workspace

# 或构建特定 crate
cargo build -p openfang-runtime
```

### 2. 运行 Benchmark

```bash
cd /home/wzx/alloyagent/alloyfang

# 运行所有基准测试
cargo bench

# 运行特定测试
cargo bench --bench cold_start     # 冷启动测试
cargo bench --bench memory_usage   # 内存使用测试
cargo bench --bench tool_execution  # 工具执行测试
cargo bench --bench reference_passing  # 引用传递测试
cargo bench --bench parallel_execution  # 并行执行测试
cargo bench --bench agent_loop     # Agent 循环测试
```

### 3. 查看结果

Benchmark 结果保存在：

- HTML 报告: `alloyfang/target/criterion/report/index.html`
- 原始数据: `alloyfang/reports/benchmark_raw.txt`

## 使用方式

### 两种运行模式

```rust
use alloyfang::AloyFangRuntime;

// AlloyFang 模式：启用所有优化
let runtime = AloyFangRuntime::optimized();

// Baseline 模式：禁用所有优化（对比用）
let runtime = AloyFangRuntime::baseline();
```

### 配置选项

```rust
use openfang_runtime::alloystack_optim::AlloyStackConfig;

let config = AlloyStackConfig {
    enable_lazy_loading: true,
    enable_reference_passing: true,
    enable_parallel_execution: true,
    max_cached_modules: 64,
    max_shared_buffer_size: 256 * 1024 * 1024, // 256MB
    max_parallel_threads: num_cpus::get(),
};
```

## 性能测试场景

### 冷启动测试 (Cold Start)

对比模块注册到首次执行的就绪时间：
- **Baseline**: 注册时预编译所有模块
- **AlloyFang**: 按需编译，仅编译实际使用的模块

### 内存使用测试 (Memory Usage)

对比不同模块数量下的内存占用：
- 统计 RSS (Resident Set Size)
- 分析 WASM 模块缓存效率

### 工具执行测试 (Tool Execution)

模拟 agent 工具调用的延迟：
- Mock LLM 驱动 (可配置延迟)
- Mock 工具 (可配置延迟)

### 引用传递测试 (Reference Passing)

测试大块数据在 agent 间传递的开销：
- 不同数据大小: 1KB ~ 10MB
- 对比拷贝 vs 零拷贝

### 并行执行测试 (Parallel Execution)

测试工作流并行度对吞吐量的影响：
- 不同任务数量: 2, 4, 8, 16
- 测量总执行时间

## 技术细节

### 核心技术

1. **LazyWasmLoader**: 基于 wasmtime 实现，模块注册与编译分离，LRU 缓存
2. **SharedBuffer**: 基于 Arc<[u8]> 实现零拷贝共享内存
3. **ParallelWorkflowExecutor**: 基于 tokio 的并发任务调度器

### 依赖关系

```
alloyfang
    │
    ├─> openfang-runtime (alloystack_optim 模块)
    │       │
    │       └─> lazy_loader.rs
    │       └─> reference_passing.rs
    │       └─> parallel_executor.rs
    │
    ├─> openfang-types
    └─> openfang-memory
```

## 贡献

欢迎提交 Issue 和 PR！请确保：

1. 运行 `cargo test --workspace` 通过所有测试
2. 运行 `cargo clippy --workspace -- -D warnings` 无警告
3. 新功能包含对应的 benchmark 测试

## 许可证

MIT License - 见各子项目 LICENSE 文件。
