<!-- CDX: CLAW review template for mentor evaluation of student output. -->

# Review Template（验收模板）

> **用途**: Mentor 审查 Student 提交的成果
> **在 TUI 中的对应**: `agent_eval` + 人工审查

---

## 标准审查格式

```markdown
<!-- CDX: mentor review. -->

## Verdict

Pass / Needs Changes / Blocked

## Findings

### 1. Severity: <BLOCKER | MAJOR | MINOR | NIT>
- File: <path:line>
- Problem: <description>
- Impact: <what breaks or is affected>
- Required fix: <specific action>

### 2. Severity: <...>
...

## Verification

<验证命令和结果>

## Evidence Check

- [ ] Boundary respected (did not touch forbidden areas)
- [ ] ≥2 evidence paths provided
- [ ] Verification command executed and results reported
- [ ] Remaining risks identified
- [ ] Long-task split applied (if applicable)

## Message to Student

```text
<可复制指令，告诉学生下一步做什么>
```
```

---

## 评分标准

| 评级 | 含义 | 后续动作 |
|------|------|---------|
| **Pass** | 目标完成，证据充分，无风险 | 关闭子代理（`agent_close`） |
| **Needs Changes** | 有 MAJOR 或 MINOR 问题 | 发送修复指令，等待学生重做 |
| **Blocked** | 有 BLOCKER 或安全违规 | 立即停止，需要人类介入 |

### 严重度定义

| 级别 | 含义 | 示例 |
|------|------|------|
| **BLOCKER** | 必须修复，阻塞后续 | 改了禁止区域、破坏性操作未确认 |
| **MAJOR** | 必须修复，不阻塞后续 | 证据不足、验证未通过 |
| **MINOR** | 建议修复 | 格式问题、小优化 |
| **NIT** | 可选改进 | 命名建议、注释补充 |

---

## 快速审查清单

```
□ 目标是否达成？
□ 是否遵守 Scope 边界？
□ 是否有 ≥2 条证据路径？
□ 是否运行了验证命令？
□ 是否报告了残留风险？
□ 长任务是否分阶段？
□ 是否有覆盖他人文件？
□ 验证结果是否如实报告（含失败）？
```
