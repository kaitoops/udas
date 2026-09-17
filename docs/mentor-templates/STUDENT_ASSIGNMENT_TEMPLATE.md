<!-- CDX: CLAW student assignment template for DeepSeek TUI Level 1 overlay. -->

# Student Assignment Template

> **使用方式**: 复制此模板到 `agent_open` 的 assignment 参数中，填充具体内容后发送
> **目标子代理**: 任何通过 `agent_open` 启动的子代理（默认 deepseek-v4-flash）

---

## Student 自检协议

每次开始执行前，子代理必须确认以下内容：

```text
[自检] 我已阅读任务分配，理解以下要素：
- Goal（目标）: <复述>
- Scope（边界）: 允许 <...>，禁止 <...>
- Evidence（证据要求）: <...>
- Stop Condition（停止条件）: <...>
```

---

## 标准任务分配模板

复制以下模板，替换 `<...>` 占位符后作为 `agent_open` 的 assignment 使用：

```markdown
<!-- CDX: mentor handoff for bounded task execution. -->

## Student Self-Check

Before starting, confirm: you understand the goal, scope, evidence requirements, and stop condition. Do not proceed until confirmed.

## Goal

<一句话描述任务目标>

## Scope

- Allowed files/systems:
  - <file/dir 1>
  - <file/dir 2>
- Do not touch:
  - <protected area 1>
  - <protected area 2>

## Required Evidence

- Evidence path 1: <如：运行 cargo check 查看编译结果>
- Evidence path 2: <如：grep 验证修改范围>

## Execution Rules

1. Complete only this phase. Do NOT continue to next phase without explicit authorization.
2. Stop before any public or destructive actions.
3. If this task requires >10 tool calls, >3 files, or >10 minutes, propose a phase split immediately.
4. Do not overwrite files you did not create unless explicitly told to.
5. Report verification results even if they are negative.

## Verification

```powershell
<验收命令>
```

## Report Back

When done, return in this format:
- Completed: <what was done>
- Files touched: <list of paths>
- Evidence: <verification results>
- Remaining risk: <known issues>
- Next phase: <suggested next step, if any>
```

---

## 含经验注入的模板（重复错误场景）

当子代理之前犯过类似错误时，在 assignment 开头追加 Lesson：

```markdown
<!-- CDX: mentor handoff with lesson injection. -->

## Lesson: <错误标题>

Symptom: <观察到的错误现象>
Root Cause: <根因>
Wrong Shortcut: <学生之前犯的错>
Checklist:
  1. <检查项 1>
  2. <检查项 2>
Verification: <如何确认已修复>

---

## Goal

<任务目标>

## Scope
...

（其余部分与标准模板相同）
```

---

## 子代理安全铁律

以下规则子代理必须遵守（无论 assignment 是否明确提及）：

1. **不静默长循环** — 长任务主动拆分，不默默跑 20 次工具调用
2. **不单一信号定根因** — 至少检查 2 条证据路径
3. **不经批准不做破坏操作** — 删除、覆盖、公开发布前必须确认
4. **不覆盖他人更改** — 不修改自己未创建的文件，除非明确指示
5. **不跳过验证** — 完成后必须运行验证命令并报告结果
6. **不幻觉报告** — 验证结果如实报告，通过和失败都要写
