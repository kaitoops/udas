# HARNESS 多 Agent 协作系统 — 快速启动指南

> **一句话**：用 YOLO 模式启动 DeepSeek TUI，让协调者自动调度多个专业 Agent 并行完成编码任务。

---

## 第一步：以 YOLO 模式启动

```bash
deepseek --yolo
```

或者先进入 TUI 再切换：

```bash
deepseek              # 进入交互式 TUI
/mode yolo            # 切换到 YOLO 模式
```

### 为什么必须用 YOLO 模式？

| 模式 | 工具审批 | 适用场景 |
|------|---------|---------|
| **Plan** | 只读，无执行 | 纯分析、调研 |
| **Agent** | 文件操作自动批准，Shell 需确认 | 轻量编辑 |
| **YOLO** | 全部自动批准 | **多 Agent 编码任务** |

HARNESS 多 Agent 系统需要协调者自由调度子代理执行文件读写、编译、测试等操作。Agent 模式下每个 Shell 操作都需要人工确认，会导致 6 个子代理的审批请求淹没主窗口。**YOLO 模式是 HARNESS 的前提条件。**

> **安全提示**：YOLO 模式会自动批准所有工具执行（包括 Shell 命令和文件写入）。仅在你信任的工作空间中使用。配合 Git 版本控制，任何误操作都可以回退。

---

## 第二步：下达编码任务

在 YOLO 模式下，直接描述你的编码需求。协调者（Coordinator）会自动：

1. **分析任务** — 将需求拆分为可并行的子任务
2. **生成合同（Contract）** — 为每个子任务定义目标、工具集、验收标准
3. **调度执行** — 通过 `agent_open` 启动子代理，每个子代理自动获得独立终端窗口

### 示例对话

```
你：重构 src/auth 模块，将 JWT 验证逻辑抽离为独立中间件，
    同时补充单元测试（覆盖率 > 80%），最后确保 cargo check 通过。
```

协调者会自动拆分为：

```
┌─────────────────────────────────────────────┐
│          协调者终端（主 TUI）                │
│                                             │
│  正在拆分任务...                              │
│                                             │
│  ✓ 合同 #1: 抽离 JWT 中间件 (generator)     │
│  ✓ 合同 #2: 重构 auth 模块引用 (generator)   │
│  ✓ 合同 #3: 编写单元测试 (generator)         │
│  ✓ 合同 #4: 验证编译 (generator)            │
│                                             │
│  已启动 4 个子代理，正在执行...               │
└─────────────────────────────────────────────┘
        │
        ├── 子代理窗口 1 [generator] — JWT 中间件
        ├── 子代理窗口 2 [generator] — 重构引用
        ├── 子代理窗口 3 [generator] — 单元测试
        └── 子代理窗口 4 [generator] — 编译验证
```

---

## 第三步：监控执行

### 多窗口实时监控

每个子代理自动弹出新终端窗口，实时显示：

| 标记 | 颜色 | 内容 |
|------|------|------|
| `[STATUS]` | 蓝色 | 进度更新（step 3/20） |
| `[THINK]` | 黄色淡色 | 推理过程（thinking blocks） |
| `[TEXT]` | 白色 | 文本输出 |
| `[TOOL]` | 紫色 | 工具调用（read_file, write_to_file...） |
| ✓ / ✗ | 绿色/红色 | 工具执行结果 |

窗口管理规则：
- **最多 6 个**并发显示窗口
- 超出 6 个的子代理写入 relay 日志（`~/.udas/agent-display/<id>/output.log`）供事后查看
- 完成后 **30 分钟自动关闭**，按 Ctrl+C 可立即关闭

### 主终端状态

协调者在主窗口持续汇报：
- 哪些子代理已启动/运行中/已完成
- 每个子代理的执行摘要（完成后自动回传）
- 任务完成后的整合结论

---

## HARNESS 角色系统

```
HARNESS = Hierarchical Agent Runtime NEgotiation & Structured Supervisor
```

### 三种 Agent 角色

| 角色 | 模型 | 职责 |
|------|------|------|
| **Planner（规划器）** | `deepseek-v4-pro` | 分析用户意图，拆分为结构化合同（Contract） |
| **Evaluator（评估器）** | `deepseek-v4-flash` | 审查合同质量，验证执行输出 |
| **Generator（生成器）** | `deepseek-v4-flash` | 执行合同，交付代码/文档/分析产物 |

### 协作流程

```
用户需求
   │
   ▼
Planner (PRO) ─── 拆分为多个 Contract
   │
   ├── Contract 1 ──→ Generator (Flash) ──→ 产物
   ├── Contract 2 ──→ Generator (Flash) ──→ 产物  ← 并行执行
   ├── Contract 3 ──→ Generator (Flash) ──→ 产物
   │
   ▼
Evaluator (Flash) ─── 审查所有产物 ───→ 通过/返工
   │
   ▼
协调者整合 ───→ 向用户交付最终结果
```

### 手动指定角色（高级用法）

你可以通过 `harness_role` 参数手动指定子代理角色：

```
agent_open(
  prompt="审查这个 PR 的代码质量和安全性",
  harness_role="evaluator"
)
```

可选值：`"planner"` | `"evaluator"` | `"generator"`

---

## 常用命令速查

### 启动

```bash
deepseek --yolo                  # YOLO 模式启动（推荐用于 HARNESS）
deepseek --yolo --workspace ./my-project   # 指定工作空间
deepseek -c --yolo               # 继续上次会话 + YOLO 模式
```

### TUI 内命令

```bash
/mode yolo                       # 切换到 YOLO 模式
/mode agent                      # 回到 Agent 模式（需要确认）
/compact                         # 压缩上下文（超过 60% 时执行）
/queue                           # 查看排队的后续任务
/model auto                      # 自动选择模型（简单任务用 Flash，复杂任务用 Pro）
```

### 子代理管理

```bash
# 以下由协调者自动调用，也支持手动使用
agent_open(prompt="...", harness_role="generator")  # 启动子代理
agent_eval(agent_id="...")                         # 评估子代理输出
agent_close(agent_id="...")                        # 关闭子代理
```

---

## 最佳实践

### 1. 任务描述要具体

```
# 好的描述
"为 src/api/routes.rs 中的 /users 端点添加分页支持，
 使用 offset/limit 参数，默认每页 20 条，最大 100 条。
 同时更新对应的单元测试。"

# 模糊的描述
"改一下用户接口"
```

### 2. 大任务自动并行

协调者会自动识别可并行的子任务。描述越完整，拆分越精准：

```
"重构支付模块：
 1. 将 Stripe 和 PayPal 的支付逻辑抽象为 PaymentProvider trait
 2. 实现工厂模式动态选择支付提供商
 3. 补充集成测试（mock 两个提供商）
 4. 确保 cargo test 和 cargo clippy 通过"
```

### 3. 上下文管理

- **每 3 轮后检查上下文使用率**，超过 60% 时执行 `/compact`
- **独立子任务交给子代理**，保持主会话精简
- 使用 `@path` 附加关键文件上下文

### 4. 会话恢复

```bash
deepseek -c --yolo    # 继续最近会话
deepseek -r <ID> --yolo  # 继续指定会话
```

---

## 故障排查

| 问题 | 解决方案 |
|------|---------|
| 子代理窗口没有弹出 | 检查 `~/.udas/agent-display/<id>/output.log` 是否存在；窗口可能超出 6 个上限 |
| 子代理卡住不动 | 检查 API 连通性：`deepseek doctor` |
| 上下文爆满 | 立即执行 `/compact`；考虑将后续任务拆到新会话 |
| 编译错误 | 子代理会自动重试；检查 relay 日志获取详细错误信息 |

---

## 相关文档

| 文档 | 内容 |
|------|------|
| [AGENT_DISPLAY_RELAY.md](docs/AGENT_DISPLAY_RELAY.md) | 多终端显示系统架构 |
| [MODES.md](docs/MODES.md) | Plan / Agent / YOLO 模式详解 |
| [SUBAGENTS.md](docs/SUBAGENTS.md) | 子代理角色分类和生命周期 |
| [ARCHITECTURE.md](docs/ARCHITECTURE.md) | 代码架构内部说明 |
