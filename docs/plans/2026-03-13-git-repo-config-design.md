# Git 仓库配置设计方案

## 项目背景

AlloyAgent 项目用于测试 AlloyFang 使用 AlloyStack 优化后的性能对比，包含多个子项目：
- `alloyfang` - 基准测试套件
- `AlloyStack` - Library OS (包含 nom-uri submodule)
- `openfang` - Agent 操作系统

## 问题分析

| 问题 | 描述 |
|------|------|
| Cargo.lock | 根 .gitignore 忽略了所有 Cargo.lock，导致依赖版本不确定 |
| zeroclaw | 用户确认不再使用，但仍在仓库中被跟踪 |
| AlloyStack submodule | nom-uri 需要用户手动初始化 |

## 解决方案

### 1. Cargo.lock 处理
- 从根 `.gitignore` 移除 `Cargo.lock` 规则
- 各子项目的 `Cargo.lock` 现在会被提交：
  - `alloyfang/Cargo.lock`
  - `AlloyStack/Cargo.lock`
  - `openfang/Cargo.lock`

### 2. zeroclaw 处理
- 从仓库中移除 zeroclaw 跟踪（`git rm --cached`）
- 从 `.gitignore` 中移除相关规则

### 3. 用户拉取后操作
```bash
# 初始化 AlloyStack 的 nom-uri submodule
git submodule update --init

# 构建项目
cd alloyfang && cargo build --release
cd openfang && cargo build --release
cd AlloyStack && cargo build
```

## 变更记录

- 2026-03-13: 初始配置
