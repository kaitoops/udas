# Sub-Agent Display Relay 架构说明

> 会话 ID: turn_bd2b97f0（HARNESS T-001 测试会话）
> 版本: 0.8.39
> 最终编译验证: cargo check --package deepseek-tui ✅ (2026-05-20)

## 概述

多终端显示系统（Multi-Terminal Display System）解决子代理运行时用户不可见的问题。当协调者通过 `agent_open` 启动子代理时，自动在独立终端窗口中以只读方式实时展示子代理的输出（思考块、文本响应、工具调用、进度）。

## 架构总览

```
协调者终端（主 TUI）
      │
      ├── agent_open → SubAgentDisplayRelay
      │                    │
      │              ┌─────┴──────┐
      │              │            │
      │        output.log   新终端窗口
      │       (ANSI 文件)   (只读显示)
      │              │
      ▼              ▼
  SubAgentManager → run_subagent_task
                       │
                       ▼
                    run_subagent()
                       │
                  ┌────┴────┐
                  │         │
            LLM 响应    工具调用
                  │         │
                  ▼         ▼
            DisplayRelay.write_*()
```

## 文件清单

### 核心新增

| 文件 | 行数 | 职责 |
|------|------|------|
| `crates/tui/src/tools/subagent/display_relay.rs` | 236 | 输出中继模块 |
| `crates/tui/src/main.rs` | +95 | `--agent-display` 子命令 + `run_agent_display()` |
| `crates/tui/src/tools/subagent/mod.rs` | +45 | 集成中继到子代理生命周期 |

### 辅助（临时，已清理）

| 文件 | 状态 |
|------|------|
| `vs_BuildTools.exe` | ❌ 已删除 |
| `dl_vs.ps1` | ❌ 已删除 |
| `build_check.bat` | ❌ 已删除 |
| `check_build.bat` | ✅ 保留（可用于 vcvars + cargo check） |

### 交付物

| 文件 | 说明 |
|------|------|
| `MARKDOWN_ARCHITECTURE.md` | HARNESS T-001 合同交付物 |
| `_DISPLAY_SYSTEM_CONTEXT.md` | **索引指针**（本文件） |
| `docs/AGENT_DISPLAY_RELAY.md` | 本构架说明文档 |

## 核心数据结构

### `SubAgentDisplayRelay`

```
crates/tui/src/tools/subagent/display_relay.rs

pub struct SubAgentDisplayRelay {
    agent_id: String,
    output_path: PathBuf,     // ~/.udas/agent-display/<id>/output.log
    done_path: PathBuf,       // ~/.udas/agent-display/<id>/done
    terminal_launched: bool,
}
```

方法：
- `write_status()` — 进度更新（蓝色 `[STATUS]`）
- `write_thinking()` — 思考块（黄色淡色 `[THINK]`）
- `write_text()` — 文本响应（`[TEXT]`）
- `write_tool_call()` — 工具调用（紫色 `[TOOL]`）
- `write_tool_result()` — 工具结果（绿色 ✓ / 红色 ✗）
- `write_complete()` / `write_failed()` — 完成/失败 + done 标记
- `launch_terminal()` — 平台自适应打开新终端

### 显示文件格式

路径: `~/.udas/agent-display/<agent-id>/output.log`

每行包含 ANSI 转义码，终端直接渲染：
```
\x1b[34m[STATUS]\x1b[0m step 3/20: requesting model response
\x1b[2m\x1b[33m[THINK]\x1b[0m\x1b[2m 分析项目结构...\x1b[0m
[TEXT] 项目有14个crate
\x1b[35m[TOOL]\x1b[0m step 2: \x1b[1mread_file\x1b[0m
```

done 标记: `~/.udas/agent-display/<agent-id>/done`（内容: "done" 或 "failed"）

## 代码集成点

### `SubAgentTask`（mod.rs ~line 3268）

新增字段:
```rust
display_relay: Option<display_relay::SubAgentDisplayRelay>,
```

### `spawn_background_with_assignment_options`（mod.rs ~line 1195）

创建 relay 并启动终端:
```rust
let mut display_relay = Some(display_relay::SubAgentDisplayRelay::new(&agent_id));
if let Some(ref mut r) = display_relay {
    r.write_status(&format!("spawned (type: {}, steps: {max_steps})", agent_type.as_str()));
    r.launch_terminal();
}
```

### `run_subagent`（mod.rs ~line 3425）

新增参数 `relay: Option<&SubAgentDisplayRelay>`，在以下位置注入：
- 收到 LLM 响应后: `ContentBlock::Thinking` → `r.write_thinking()`，`ContentBlock::Text` → `r.write_text()`
- 工具调用时: `r.write_tool_call()` / `r.write_tool_result()`

### `run_subagent_task`（mod.rs ~line 3280）

- 任务开始: `r.write_status("started")`
- 任务完成: `r.write_complete(&summary)`
- 任务失败: `r.write_failed(&err)`

### `--agent-display` 子命令（main.rs ~line 1010）

```rust
fn run_agent_display(agent_id: &str) -> Result<()>
```

- 读取 `~/.udas/agent-display/<agent-id>/output.log`
- 轮询新内容（300ms 间隔），增量输出
- 检测 done 标记后显示最终信息并停留（等待 Ctrl+C）
- 不读取 stdin（只读模式）

### 平台终端启动（display_relay.rs ~line 161）

| 平台 | 命令 |
|------|------|
| Windows | `start "Agent <id>" cmd /c "deepseek-tui agent-display <id>"` |
| macOS | `open -a Terminal deepseek-tui --args agent-display <id>` |
| Linux | `x-terminal-emulator -e "deepseek-tui agent-display <id>"` |

## 涉及的改动范围

### 新增文件
- `crates/tui/src/tools/subagent/display_relay.rs` — 核心中继

### 修改文件
- `crates/tui/src/tools/subagent/mod.rs` — 集成点（SubAgentTask, spawn, run_subagent_task, run_subagent）
- `crates/tui/src/main.rs` — 显示子命令 + 终端客户端

### 影响面
- 子代理系统: 所有通过 `agent_open` / `agent_spawn` 启动的子代理自动获得显示终端
- 性能: 文件 I/O + 300ms 轮询，对子代理运行无阻塞影响
- 失败模式: relay 创建失败或终端启动失败均为静默降级，不影响子代理正常执行

## 验证方法

```bash
# 完整编译检查
call "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvars64.bat"
cargo check --package deepseek-tui

# 手动测试显示客户端
cargo run --package deepseek-tui -- agent-display <agent-id>
```
