<!-- CDX: CLAW handoff template adapted for DeepSeek TUI agent_open. -->

# Handoff Template（任务交接文档）

> **用途**: Mentor 向 Student 传递任务的标准格式
> **在 TUI 中的对应**: `agent_open` 的 `assignment` 参数

---

## 标准格式

```markdown
<!-- CDX: mentor handoff. -->

## Goal

<一句话目标>

## Target

- Student: <子代理标识，如 DST/flash>
- Type: <General/Explore/Review/Implementer>
- Model: <deepseek-v4-flash / deepseek-v4-pro>

## Scope

- Allowed:
  - <file/system 1>
  - <file/system 2>
- Do not touch:
  - <protected area>

## Required Evidence

- Evidence path 1: <具体验证方式>
- Evidence path 2: <独立交叉验证>

## Execution Rules

1. Complete only this phase.
2. Stop before public/destructive actions.
3. Long-task threshold: >10 calls / >3 files / >10min → propose split.
4. Do not overwrite existing files unless told to.

## Verification

<命令或验收标准>

## Report Back

- Completed: <what changed>
- Files touched: <paths>
- Evidence: <verification results>
- Remaining risk: <issues>
- Next phase: <suggestion>
```

---

## 分阶段任务的 Handoff 格式

当任务需要拆分为多个阶段时：

```markdown
<!-- CDX: mentor multi-phase handoff. -->

## Goal

<总体目标>

## Phase Plan

- Phase 1: <描述> (current)
- Phase 2: <描述> (blocked)
- Phase 3: <描述> (blocked)

## Current Phase: 1 — <标题>

### Scope
- Allowed: <...>
- Do not touch: <...>

### Required Evidence
- <...>

### Verification
<...>

### Report Back
<同标准格式>

---
BLOCKED: Phase 2 and 3 require Mentor/Owner approval after Phase 1 completion.
```

---

## 高风险任务的 Handoff 格式

涉及公开操作、删除、跨模块变更时：

```markdown
<!-- CDX: mentor high-risk handoff. -->

## Risk Level: HIGH

### Risk Assessment
- Type: <public post / file deletion / multi-module change / credentials>
- Impact: <描述潜在影响>
- Reversible: <yes/no>

## Phase 1: Dry-Run / Impact Assessment Only

### Scope
- READ ONLY. Do not modify any files.
- List what would change, but do not change it.

### Required Evidence
- <impact analysis>
- <affected files list>

### Verification
<preview/review commands>

### Report Back
<同标准格式>

---
BLOCKED: Execution phase requires explicit Owner approval.
```
