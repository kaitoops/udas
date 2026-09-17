<!-- CDX: CLAW lesson template for experience injection in DeepSeek TUI. -->

# Lesson Template（经验模板）

> **用途**: 记录子代理犯错后的经验教训，用于下次注入到 assignment 中
> **存储位置**: `~/.udas/lessons/`（Level 1 手动管理）

---

## 经验文件格式

```markdown
<!-- CDX: mentor-created lesson after observed failure. -->

# <标题>

Created: <ISO 8601 datetime>
Context: <什么场景下触发的这个经验>

## Symptom

<观察到的错误现象，学生做了什么导致问题>

## Root Cause

<窄因：具体的技术原因，不是泛泛的"代码有问题">

## Evidence

- Evidence path 1: <证明根因的第一条证据>
- Evidence path 2: <交叉验证的第二条证据>

## Wrong Shortcut To Avoid

<学生常见的错误捷径，如"看到 X 错误就直接改 Y">
<解释为什么这个捷径是错的>

## Checklist

1. <检查项 1：下次遇到类似问题首先检查什么>
2. <检查项 2：排除什么>
3. <检查项 3：确认什么>

## Verification

<如何验证这个问题已经被正确修复>
```

---

## 经验注入方式

在 `agent_open` 的 assignment 开头直接插入：

```markdown
<!-- CDX: lesson injection from ~/.udas/lessons/ -->

## Lesson: <标题>

Symptom: <...>
Root Cause: <...>
Wrong Shortcut: <...>
Checklist:
  1. <...>
  2. <...>
Verification: <...>

---

（以下接正常的 Goal/Scope/Evidence/...）
```

---

## 何时创建 Lesson

| 触发条件 | 示例 |
|---------|------|
| 学生误判根因 | "说是 API key 问题，实际是 URL 拼写错误" |
| 修完一处又破坏另一处 | "修了编译错误但破坏了显示系统" |
| 重复犯同一个错误 | 第二次忘记检查文件编码 |
| 跳过验证步骤 | "说改完了但没跑 cargo check" |
| 需要新的安全边界 | "差点删了用户文件" |

---

## 经验文件命名规范

```
~/.udas/lessons/
  2026-05-22-single-signal-root-cause.md
  2026-05-22-encoding-utf8-gbk.md
  2026-05-22-skip-verification.md
```

格式：`YYYY-MM-DD-<slug>.md`

slug 使用英文小写 + 连字符。
