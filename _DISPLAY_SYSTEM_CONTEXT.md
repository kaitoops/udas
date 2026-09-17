# Multi-Terminal Display System — Index Pointer

> 当涉及以下文件时，必须预先载入 `docs/AGENT_DISPLAY_RELAY.md` 作为上下文

## 触发路径

```
crates/tui/src/tools/subagent/display_relay.rs
crates/tui/src/tools/subagent/mod.rs         (display_relay 相关部分)
crates/tui/src/main.rs                       (agent-display 子命令部分)
```

## 触发修改场景

- 子代理输出可视化
- 多终端显示
- SubAgentDisplayRelay
- agent-display 子命令
- 终端窗口自动启动
- 子代理 thinking/推理内容显示
- 只读终端客户端

## 关联文件

- 文档: `docs/AGENT_DISPLAY_RELAY.md`
- 索引指针本体: `_DISPLAY_SYSTEM_CONTEXT.md`
- 项目指令（Session 迁移规则）: `AGENTS.md` → 搜索 `Multi-Terminal Display System`
- 子代理系统: `crates/tui/src/tools/subagent/mod.rs`
- 输出中继: `crates/tui/src/tools/subagent/display_relay.rs`
- 显示客户端: `crates/tui/src/main.rs`（`fn run_agent_display`）

## Session 迁移要求

每次新 Session 开启时，必须确保：
1. 预载本文件 `_DISPLAY_SYSTEM_CONTEXT.md` 和 `docs/AGENT_DISPLAY_RELAY.md`
2. 验证 `crates/tui/src/tools/subagent/display_relay.rs` 存在且模块声明完整
3. 确认 `AGENTS.md` 中的 `Multi-Terminal Display System` 节已载入上下文
