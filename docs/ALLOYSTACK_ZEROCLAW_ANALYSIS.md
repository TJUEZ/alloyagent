# AlloyStack + Zeroclaw 架构分析与集成方案

## 一、AlloyStack 核心原理总结

### 1.1 系统定位

AlloyStack 是一个面向 **Serverless Workflow** 的 **Library OS (LibOS)**，发表于 EuroSys 2025。其核心目标是：
- **降低冷启动延迟**：通过按需加载 (on-demand loading)
- **减少中间数据传输开销**：通过引用传递 (reference passing)

### 1.2 架构分层

```
┌──────────────────────────────────────────────────────────────┐
│                      asvisor (监管器)                         │
│        bins/asvisor/src/main.rs - 工作流编排入口              │
├──────────────────────────────────────────────────────────────┤
│                    libasvisor (核心运行时)                    │
│  ├─ Isolation (隔离域管理)                                   │
│  ├─ Service/ServiceLoader (服务加载器，dlmopen命名空间隔离)    │
│  ├─ MetricBucket (性能指标收集)                               │
│  └─ MPK (Intel Memory Protection Keys 内存保护)              │
├──────────────────────────────────────────────────────────────┤
│                      as_std (用户态标准库)                    │
│  ├─ libos 宏 (hostcall 封装)                                 │
│  ├─ heap_alloc (服务堆分配器)                                │
│  └─ mpk (MPK权限切换)                                        │
├──────────────────────────────────────────────────────────────┤
│                   common_service (LibOS模块)                  │
│  ├─ mm (内存管理，含faas_buffer共享缓冲区)                    │
│  ├─ fatfs (文件系统)                                         │
│  ├─ smoltcp (网络栈)                                         │
│  └─ 其他LibOS服务模块                                        │
├──────────────────────────────────────────────────────────────┤
│                   as_hostcall (主机调用接口)                  │
│  定义跨隔离域调用的类型和协议                                  │
└──────────────────────────────────────────────────────────────┘
```

### 1.3 核心机制详解

#### 1.3.1 Namespace 隔离 (dlmopen)

**文件**: `libasvisor/src/service/loader.rs:158-192`

```rust
fn do_dlmopen(filename: &Path, lmid: Option<Lmid_t>) -> Result<*mut c_void> {
    let handle = unsafe {
        nix::libc::dlmopen(
            lmid.unwrap_or(nix::libc::LM_ID_NEWLM),  // 新建或复用命名空间
            filename.as_ptr(),
            nix::libc::RTLD_LAZY | nix::libc::RTLD_LOCAL,
        )
    };
    // ...
}
```

**优势**：
- 每个函数实例有独立的符号表和全局变量
- 避免动态链接器的全局锁竞争
- 支持同一函数的多实例并行执行

#### 1.3.2 引用传递 (Reference Passing)

**文件**: `common_service/mm/src/faas_buffer/mod.rs`

```rust
// 共享缓冲区注册表 - 跨函数共享数据
static ref BUFFER_REGISTER: Mutex<HashMap<String, (usize, u64)>> = ...;

// 分配共享缓冲区
pub fn buffer_alloc(slot: &str, layout: Layout, fingerprint: u64) -> MMResult<usize> {
    let addr = BUFFER_ALLOCATOR.lock().allocate_first_fit(layout)?;
    BUFFER_REGISTER.lock().insert(slot.to_owned(), (addr, fingerprint));
    Ok(addr)
}

// 访问共享缓冲区（零拷贝）
pub fn access_buffer(slot: &str) -> Option<(usize, u64)> {
    BUFFER_REGISTER.lock().remove(slot)
}
```

**工作原理**：
1. 函数 A 调用 `buffer_alloc("output", layout, fingerprint)` 分配缓冲区
2. 函数 A 直接写入数据到缓冲区
3. 函数 B 调用 `access_buffer("output")` 获取缓冲区地址
4. 函数 B **直接读取**，无需数据拷贝

**性能收益**：消除了传统 Serverless 中通过 Redis/S3 等中间件传递数据的序列化/网络开销。

#### 1.3.3 HostCall 机制

**文件**: `as_std/src/libos/mod.rs` + `as_std/src/libos/utils.rs`

```rust
// HostCall表：缓存跨域调用入口
pub struct UserHostCall {
    metric_addr: Option<usize>,
    alloc_buffer_addr: Option<usize>,
    access_buffer_addr: Option<usize>,
    // ... 其他系统调用入口
}

// libos! 宏：高效调用LibOS服务
pub macro libos {
    ($name:ident($($arg)*)) => {
        {
            let func: FuncType = transmute(table.get_or_find(hostcall_id!($name)));
            func($($arg)*)
        }
    }
}
```

**设计亮点**：
- **懒查找**：首次调用时查找函数地址，后续直接使用缓存
- **类型安全**：通过宏生成正确的函数签名
- **MPK集成**：调用前后自动切换内存保护密钥

#### 1.3.4 MPK 内存保护

**文件**: `as_std/src/libos/utils.rs:99-142`

```rust
pub macro libos_with_switch_mpk {
    ($name:ident($($arg)*)) => {
        {
            let pkru = mpk::pkey_read();
            let is_privilege = (pkru >> 30 == 0);

            // 授予LibOS访问权限
            let pkru = mpk::grant_libos_perm(pkru);
            asm!("wrpkru", in("rax") pkru);

            let result = func($($arg)*);

            // 撤销LibOS访问权限
            if !is_privilege {
                let pkru = mpk::drop_libos_perm(pkru);
                asm!("wrpkru", in("rax") pkru);
            }
            result
        }
    }
}
```

**安全边界**：
- 用户函数无法直接访问 LibOS 内存区域
- 仅通过 hostcall 临时获得访问权限
- 硬件级别的内存隔离，开销约 20 CPU cycles

### 1.4 性能优势分析

| 优化点 | 传统Serverless | AlloyStack | 收益 |
|-------|---------------|------------|------|
| 冷启动 | 加载完整运行时 | 按需加载模块 | ~5x 延迟降低 |
| 数据传递 | 序列化→网络→反序列化 | 直接内存引用 | ~100x (256MB数据) |
| 函数隔离 | 进程/容器边界 | dlmopen命名空间 | 更轻量 |
| 内存保护 | 进程隔离 | MPK | 微秒级切换 |

---

## 二、Zeroclaw 核心原理总结

### 2.1 系统定位

Zeroclaw 是一个 **100% Rust** 的 AI Agent 运行时框架，设计理念为"零开销、零妥协、100% 可插拔"。

### 2.2 架构分层

```
┌─────────────────────────────────────────────────────────────┐
│                     Agent Orchestrator                       │
│              (agent.rs + loop_.rs + dispatcher.rs)           │
├────────────┬────────────┬────────────┬────────────┬─────────┤
│  Provider  │   Memory   │   Tools    │  Security  │ Observer│
│  (LLM API) │  (SQLite)  │  (Shell等) │  (沙箱)    │ (监控)  │
└────────────┴────────────┴────────────┴────────────┴─────────┘
```

### 2.3 关键瓶颈分析

#### 2.3.1 记忆操作瓶颈

**文件**: `zeroclaw/src/memory/sqlite.rs`

| 操作 | 瓶颈点 | 影响 |
|-----|-------|------|
| `store()` | Embedding API 调用 | 500ms+ 网络延迟 |
| `recall()` | 向量全表扫描 O(n) | 大数据集性能退化 |
| `recall()` | `spawn_blocking` 跨线程 | 数据克隆开销 |

**当前优化**：
- Embedding 缓存 (LRU, max=10000)
- SQLite WAL 模式
- 批量查询避免 N+1

#### 2.3.2 工具调用瓶颈

**文件**: `zeroclaw/src/tools/shell.rs`

| 操作 | 瓶颈点 | 影响 |
|-----|-------|------|
| Shell 执行 | fork+exec 每次调用 | 10-50ms 启动开销 |
| 工具串行 | `parallel_tools=false` 默认 | 多工具耗时叠加 |
| 超时等待 | 60秒硬超时 | 阻塞整个 turn |

---

## 三、集成方案设计（Serverless 思想）

### 3.1 设计理念

借鉴 Serverless 的核心思想：
1. **函数即服务**：将 Agent 的各个组件作为独立"函数"
2. **按需实例化**：冷启动时只加载必需模块
3. **事件驱动**：通过引用传递实现组件间零拷贝通信
4. **弹性伸缩**：支持多 Agent 实例并行运行

### 3.2 架构映射

```
┌─────────────────────────────────────────────────────────────────┐
│                    AgentOS (AlloyStack 扩展)                     │
├─────────────────────────────────────────────────────────────────┤
│  Agent Isolation                                                 │
│  ├─ agent_1 (namespace_1)                                       │
│  │   ├─ provider_module (LLM 调用)                              │
│  │   ├─ memory_module (向量存储)                                │
│  │   └─ tool_modules[] (按需加载)                               │
│  ├─ agent_2 (namespace_2)                                       │
│  └─ ... (多Agent并行)                                           │
├─────────────────────────────────────────────────────────────────┤
│  Shared Services (LibOS层, MPK保护)                             │
│  ├─ embedding_service (共享嵌入计算)                            │
│  ├─ vector_index_service (共享向量索引)                         │
│  ├─ shell_pool_service (Shell会话池)                            │
│  └─ faas_buffer (Agent间数据共享)                               │
├─────────────────────────────────────────────────────────────────┤
│  asvisor (AgentOS 调度器)                                        │
│  ├─ agent_scheduler (Agent 调度)                                │
│  ├─ memory_manager (共享内存管理)                               │
│  └─ metric_collector (性能监控)                                 │
└─────────────────────────────────────────────────────────────────┘
```

### 3.3 组件适配方案

#### 方案 C: Serverless 函数化 Agent 组件

```rust
// Agent 组件作为 AlloyStack 服务
pub trait AgentComponent: Send + Sync {
    fn name(&self) -> &str;
    fn init(&self, isol_id: IsolationID) -> Result<()>;
    fn execute(&self, input: &SharedBuffer) -> Result<SharedBuffer>;
}

// 示例：Memory 组件
pub struct MemoryService {
    // 使用 AlloyStack 的 faas_buffer 进行零拷贝
    inner: Arc<SqliteMemory>,
}

impl AgentComponent for MemoryService {
    fn execute(&self, input: &SharedBuffer) -> Result<SharedBuffer> {
        // 直接从共享缓冲区读取查询
        let query: MemoryQuery = input.deserialize()?;

        // 执行查询
        let results = self.inner.recall(&query.text, query.limit).await?;

        // 结果写入共享缓冲区（零拷贝返回）
        let output = SharedBuffer::alloc("memory_result", results.size())?;
        output.serialize(&results)?;
        Ok(output)
    }
}
```

### 3.4 性能提升预期

| 优化点 | 当前 (Zeroclaw) | 集成后 (AgentOS) | 预期提升 |
|-------|-----------------|------------------|---------|
| Agent 启动 | 完整加载所有模块 | 按需加载 | 2-5x |
| 多 Agent 数据共享 | 序列化/反序列化 | 引用传递 | 10-100x |
| 工具并发 | Tokio 任务 | 独立 namespace | 隔离性更好 |
| 内存保护 | 无 | MPK | 安全性提升 |

---

## 四、测试计划

### 4.1 基准测试项目

#### 4.1.1 记忆操作测试

```rust
// 测试场景：
// 1. 单 Agent 记忆 store/recall 延迟
// 2. 多 Agent 并发记忆操作
// 3. 大数据集 (10K/100K entries) 向量搜索

#[bench]
fn memory_store_latency() {
    // 测量单次 store 延迟分布
}

#[bench]
fn memory_recall_latency() {
    // 测量单次 recall 延迟分布
}

#[bench]
fn multi_agent_memory_contention() {
    // 测量多 Agent 并发时的性能退化
}
```

#### 4.1.2 工具调用测试

```rust
// 测试场景：
// 1. 单工具调用延迟 (Shell, HTTP, etc.)
// 2. 多工具并行调用
// 3. 工具链串行调用

#[bench]
fn shell_tool_latency() {
    // 测量 shell 命令执行延迟
}

#[bench]
fn parallel_tools_throughput() {
    // 测量并行工具调用吞吐量
}
```

### 4.2 集成后对比测试

| 测试项 | 指标 | Zeroclaw 基线 | AgentOS 集成 |
|-------|------|--------------|--------------|
| 冷启动 | 首次响应延迟 | TBD ms | TBD ms |
| 记忆写入 | P99 延迟 | TBD ms | TBD ms |
| 记忆检索 | P99 延迟 | TBD ms | TBD ms |
| 多Agent | 并发吞吐 | TBD ops/s | TBD ops/s |
| 数据共享 | 传输带宽 | TBD MB/s | TBD MB/s |

### 4.3 监控指标

```rust
// 需要收集的指标
struct AgentMetrics {
    // 延迟指标
    turn_latency_ms: Histogram,
    memory_op_latency_ms: Histogram,
    tool_call_latency_ms: Histogram,

    // 资源指标
    memory_usage_bytes: Gauge,
    cpu_usage_percent: Gauge,

    // 通信指标
    ipc_latency_us: Histogram,
    zero_copy_transfers: Counter,
    data_copy_bytes: Counter,
}
```

---

## 五、实施路线图

### Phase 1: 基准测试 (当前)
- [ ] 建立 Zeroclaw 性能基准
- [ ] 记忆操作延迟测试
- [ ] 工具调用延迟测试
- [ ] 确定瓶颈点

### Phase 2: 适配器开发
- [ ] 设计 AgentComponent trait
- [ ] 实现 Memory 适配器
- [ ] 实现 Tool 适配器
- [ ] 集成 faas_buffer

### Phase 3: 集成测试
- [ ] 单 Agent 集成验证
- [ ] 多 Agent 并发测试
- [ ] 性能对比分析

### Phase 4: 优化迭代
- [ ] 根据测试结果优化
- [ ] 文档和示例完善

---

## 六、关键文件索引

### AlloyStack
```
libasvisor/src/isolation/mod.rs      - Isolation 管理
libasvisor/src/service/loader.rs     - dlmopen 服务加载
libasvisor/src/metric.rs             - 性能指标收集
common_service/mm/src/faas_buffer/   - 共享缓冲区
as_std/src/libos/                    - HostCall 机制
bins/asvisor/src/main.rs             - 入口点
```

### Zeroclaw
```
zeroclaw/src/agent/agent.rs          - Agent 主体
zeroclaw/src/memory/sqlite.rs        - SQLite 记忆后端
zeroclaw/src/tools/shell.rs          - Shell 工具
zeroclaw/src/agent/dispatcher.rs     - 工具调用协议
```
