# GitHub Trending 调研报告 — 2026-06-23

- 数据来源：GitHub API Search（2026年6月1日后创建，Stars > 500）
- 数据采集：2026-06-23 15:00 UTC
- 子Agent深度评估：5 个项目

---

## 一、总览

本期覆盖 2026 年 6 月上旬至中旬期间创建的 25 个高星项目（Stars > 500）。经质量筛选，22 个有效项目进入 Top 20 榜单，12 个项目因描述缺失/无许可证/疑似刷量/归档等原因被跳过。

**核心趋势：**
- **AI Agent 赛道占比超 40%**：从思维引导（ponytail）到编排框架（omnigent）到技能生态（BuilderIO/skills, improve）
- **大厂集体入场**：小米 MiMo-Code（10,461★）、百度 Unlimited-OCR（2,286★）、京东 JoyAI-Echo（1,662★）、Vercel eve（2,382★）
- **Agent 效率成焦点**：ponytail（-54% 代码 -20% 成本）与 improve（贵模型规划→便宜模型执行）
- **Skills 标准化**：Agent Skills 格式成为事实标准
- **多模态文档突破**：百度 Unlimited-OCR 开启零切片长文档解析

---

## 二、Top 20 榜单（按 Stars 排序）

| # | 项目 | Stars | Forks | 语言 | 许可证 | 简介 |
|---|------|-------|-------|------|--------|------|
| 1 | [ponytail](https://github.com/DietrichGebert/ponytail) | 51,684 | 2,562 | JavaScript | MIT | AI Agent 懒人开发思维引导框架 |
| 2 | [MiMo-Code](https://github.com/XiaomiMiMo/MiMo-Code) | 10,461 | 980 | TypeScript | MIT | 小米：模型与 Agent 共演化平台 |
| 3 | [shadcn/improve](https://github.com/shadcn/improve) | 6,036 | 243 | — | MIT | 最强模型审计→生成计划→便宜模型执行 |
| 4 | [astrid/book](https://github.com/unicity-astrid/book) | 5,383 | 22 | Perl | Apache-2.0 | Astrid OS 内核/胶囊/ABI 参考书 |
| 5 | [astrid/handbook](https://github.com/unicity-astrid/handbook) | 5,253 | 31 | — | Apache-2.0 | Astrid OS 开发工作手册 |
| 6 | [omnigent](https://github.com/omnigent-ai/omnigent) | 4,528 | 525 | Python | Apache-2.0 | 开源多 Agent 元编排框架 |
| 7 | [lottie](https://github.com/diffusionstudio/lottie) | 3,673 | 201 | TypeScript | MIT | AI 生成生产级 Lottie 动画 |
| 8 | [skylight](https://github.com/cpaczek/skylight) | 2,849 | 320 | TypeScript | MIT | RTL-SDR 实时飞机天花板投影 |
| 9 | [BuilderIO/skills](https://github.com/BuilderIO/skills) | 2,491 | 130 | JavaScript | MIT | 编码 Agent 技能原生格式 |
| 10 | [vercel/eve](https://github.com/vercel/eve) | 2,382 | 176 | TypeScript | Apache-2.0 | Vercel 文件系统优先 Agent 框架 |
| 11 | [kage](https://github.com/tamnd/kage) | 2,329 | 77 | Go | MIT | 离线网站镜像（去 JS） |
| 12 | [Unlimited-OCR](https://github.com/baidu/Unlimited-OCR) | 2,286 | 142 | Python | MIT | 百度不限长度文档 OCR |
| 13 | [devspace](https://github.com/Waishnav/devspace) | 2,265 | 235 | TypeScript | 有 | ChatGPT 转 Codex 体验 |
| 14 | [noop](https://github.com/NoopApp/noop) | 1,909 | 769 | Swift | 有 | 离线 WHOOP 蓝牙伴侣 |
| 15 | [baoyu-design](https://github.com/JimLiu/baoyu-design) | 1,821 | 131 | JavaScript | 有 | 本地 Claude Design UI 原型 |
| 16 | [aur-malware-check](https://github.com/lenucksi/aur-malware-check) | 1,726 | 38 | Python | — | AUR 供应链攻击检测 |
| 17 | [JoyAI-Echo](https://github.com/jd-opensource/JoyAI-Echo) | 1,662 | 149 | Python | 有 | 京东长音频视觉生成 |
| 18 | [enableMacosAI](https://github.com/SkyBlue997/enableMacosAI) | 1,520 | 83 | Shell | — | 国行 Mac 一键开启 Apple 智能 |
| 19 | [nub](https://github.com/nubjs/nub) | 1,418 | 13 | Rust | 有 | 全功能高性能 Node.js 工具包 |
| 20 | [loop-library](https://github.com/Forward-Future/loop-library) | 1,379 | 110 | JavaScript | 有 | AI Agent 实用循环库 |

---

## 三、深度评估（5 个项目）

---

### 1. ponytail — AI Agent 思维引导框架

**基本信息**
- 仓库：DietrichGebert/ponytail
- Stars：51,684 | Forks：2,562 | 比例：20.2
- 语言：JavaScript | 许可证：MIT
- 创建：2026-06-12 | 持续活跃更新

**架构分析**
ponytail 不是传统代码库，而是一套 AI Agent 行为规则集（Agent Skill），通过六层决策阶梯约束 Agent 的编码行为。核心逻辑定义在 `AGENTS.md` 中，配合 `.github/copilot-instructions.md` 等文件提供多平台适配。支撑平台包括 Claude Code、Codex CLI、Cursor、Gemini CLI、GitHub Copilot CLI、OpenClaw 等 14 种 AI 编码工具。

**核心特性**
- **六层决策阶梯**：stdlib → Platform → Deps → One-liner → Mini → Full，引导 Agent 逐层选择最简实现方式
- **安全护栏**：输入验证、错误处理、安全、可访问性等不允许"偷懒"
- **量化收益**：实测减少 54%~94% 代码量、降低 20%~77% 成本、提速 27%~6 倍
- **交互命令**：`/ponytail`（模式切换）、`/ponytail-review`（审查 diff）、`/ponytail-audit`（全库审计）
- **多平台适配**：4.8.0 版已支持 MCP Server

**活跃度**
项目发布仅 11 天即获 51k+ stars，77 个开放 Issues，125 个订阅者，社区热度极高。Hacker News、Reddit、Medium 广泛报道。

**匹配度**
- AI Agent ★★★★★ | AI框架 ★★★★ | 开发者工具 ★★★★★ | 开源创新 ★★★★★

**纠错检查** ✅ 项目名准确，归属 DietrichGebert（个人），描述与实际一致

**维护状态** ✅ 非归档，持续推送。推荐指数：9.5/10

---

### 2. omnigent — 开源多 Agent 元编排框架

**基本信息**
- 仓库：omnigent-ai/omnigent
- Stars：4,528 | Forks：525 | 比例：8.6
- 语言：Python | 许可证：Apache-2.0
- 创建：2026-06-11 | 持续活跃

**架构分析**
omnigent 定位为 meta-harness（元编排层），位于多个 Agent 之上的统一控制平面。由 Databricks 开源，Matei Zaharia 领导开发。架构分五层：Harness 适配层（封装 Claude Code/Codex/Cursor/Pi）、策略引擎（YAML 声明式安全与治理策略）、沙箱隔离（bwrap OS 级沙箱）、实时协作层、接口层（CLI/Web UI/REST API）。

**核心特性**
- 多 Agent 编排：同时运行 Claude Code 和 Codex 让它们协同或辩论
- 跨设备会话跟随：终端/Web UI/手机间无缝切换
- Agent YAML 规范：声明式定义行为、工具、策略和模型
- 细粒度策略引擎：替代传统 prompt-based 护栏
- 零云依赖：可在本地完全离线运行

**技术栈**
Python 主体，uv 包管理，bwrap 沙箱，可选 Databricks Unity AI Gateway 后端

**活跃度**
Databricks 正式开源，GitHub Trending Python 类第 2。alpha 阶段（0.2.0.dev0），但有完整贡献指南和活跃社区。

**匹配度**
- AI Agent ★★★★★ | AI框架 ★★★★★ | Agent编排 ★★★★★ | 开源创新 ★★★★★

**纠错检查** ✅ 项目名准确，归属 omnigent-ai 组织，描述与实际一致

**维护状态** ✅ 非归档，alpha 阶段但持续迭代。推荐指数：9.0/10

---

### 3. shadcn/improve — 模型级联审计执行框架

**基本信息**
- 仓库：shadcn/improve
- Stars：6,036 | Forks：243 | 比例：24.8
- 语言：无（Agent Skill） | 许可证：MIT
- 创建：2026-06-10 | 作者：shadcn（shadcn/ui 创始人）

**架构分析**
improve 是 Agent Skill 格式的模型级联框架。核心流程：最贵模型（如 Claude Opus 级）审计代码库 → 生成结构化发现项 → 产出自包含 Markdown 计划 → 便宜模型执行计划 → 回环验证。

审计覆盖 9 大类别：Bugs、安全、性能、测试覆盖率、技术债、依赖、开发者体验、文档、产品方向。支持 full/branch/quick/deep/security 等审计模式。

**核心特性**
- **分层模型协作**：贵模型理解+规划，便宜模型执行，显著降成本
- **自包含计划**：每个计划包含完整上下文、精确文件路径、验证命令
- **硬边界**：绝不修改源码自身、绝不运行工作区突变命令
- **闭环机制**：execute → reconcile → approve/revise/block
- **自愈循环**：reconcile 命令检查执行结果，修正偏差

**技术栈**
纯 Markdown 格式，兼容任何支持 Agent Skills 格式的平台（Claude Code/Cursor/Codex）

**活跃度**
6,036 stars 增长快，7 个开放 Issues。最后更新 6月15日，shadcn 的项目通常以稳定版本发布。

**匹配度**
- AI Agent ★★★★★ | AI框架 ★★★★ | 开发者工具 ★★★★★ | 开源创新 ★★★★★

**纠错检查** ✅ 项目名准确，归属 shadcn，描述与实际一致

**维护状态** ✅ 非归档，活跃项目。推荐指数：8.5/10

---

### 4. vercel/eve — Vercel Agent 构建框架

**基本信息**
- 仓库：vercel/eve
- Stars：2,382 | Forks：176 | 比例：13.5
- 语言：TypeScript | 许可证：Apache-2.0
- 创建：2026-06-16 | Vercel 官方项目

**架构分析**
eve 是 Vercel 在 Ship 26 大会上发布的文件系统优先（filesystem-first）Agent 框架。理念：一个 Agent = 一个目录。目录结构：`agent/instructions.md`（系统提示）、`agent/agent.ts`（模型配置）、`agent/tools/`（TypeScript 工具定义）、`agent/skills/`、`agent/channels/`、`agent/schedules/`。

**核心特性**
- Durable Execution：基于 Vercel Workflow SDK，状态可持久化
- Sandboxed Compute：隔离沙箱（本地 Docker / 部署 Vercel 沙箱）
- Human-in-the-Loop：人工审批工作流
- 子 Agent：独立目录，自有 instructions/tools/model/沙箱
- 多渠道：HTTP/Slack/Discord/Teams/Telegram/GitHub/Linear
- Tracing & Evals 内置

**技术栈**
TypeScript 全栈，Vercel AI Gateway，Vercel Workflow SDK，Zod 校验，Turbo + pnpm

**活跃度**
发布仅 6 天获 2,382 stars，36+ commits，77 个 Issues。Vercel 内部已有 100+ Agent 运行在 eve 上。

**匹配度**
- AI Agent ★★★★★ | AI框架 ★★★★★ | 开发者工具 ★★★★★ | 开源创新 ★★★★

**纠错检查** ✅ 项目名准确，归属 Vercel，描述与实际一致。当前 beta 阶段

**维护状态** ✅ 非归档，beta 阶段，Vercel 官方长期维护。推荐指数：8.5/10

---

### 5. baidu/Unlimited-OCR — 不限长度文档解析 OCR 框架

**基本信息**
- 仓库：baidu/Unlimited-OCR
- Stars：2,286 | Forks：142 | 比例：16.1
- 语言：Python | 许可证：MIT
- 创建：2026-06-18 | arXiv 论文：2606.23050

**架构分析**
百度基于参考滑动窗口注意力（R-SWA）的端到端文档解析模型，总参数量 3B（MoE 架构，实际激活约 500M）。R-SWA 机制模拟人类抄书的注意力模式——固定 KV 缓存为恒定容量队列，使内存占用不随输出长度增长。配合 DeepEncoder（1024×1024 图像压缩为 256 个视觉 token，16 倍压缩率），可在标准 32K 上下文窗口内一次解析数十页文档。

**核心特性**
- **一次解析不限长度**：不分页、不切片
- **双模式**：gunda（精确）和 base（基础）图像处理
- **OmniDocBench SOTA**：v1.5 93.23%，v1.6 93.92%
- **推理速度**：6144 token 输出时 TPS 7847，比 DeepSeek OCR 快 35%
- **批量推理管线**：SSE 流式输出，支持 PDF/图片目录并发处理
- **模型开源**：HuggingFace + ModelScope

**技术栈**
Python + PyTorch，HuggingFace Transformers，SG-Lang 推理引擎，Docker 部署

**活跃度**
5 天 2,286 stars，8 个 Issues，arXiv 论文同步。百度官方维护。

**匹配度**
- 多模态AI ★★★★★ | 开源创新 ★★★★★ | 开发者工具 ★★★★

**纠错检查** ✅ 项目名 Unlimited-OCR 准确，归属 baidu 组织，描述与论文一致

**维护状态** ✅ 非归档，百度官方维护，有论文支撑。推荐指数：8.0/10

---

## 四、被跳过项目列表

| 项目 | 跳过原因 |
|------|----------|
| zhongerxin/Cowart | 无描述、无许可证 |
| kanavtwtgg/birds.cafe | 0 Forks、无描述 |
| MstKail/polymarket-trading-bot-* | Forks >> Stars，疑似刷量 |
| MstKail/wc2026-crypto-sportsbook | Forks >> Stars，疑似刷量 |
| nnecrkvenuOX/formcms | 用户名随机，Forks >> Stars |
| ReulgeApmpetty0O/Back-End-Dev-* | 用户名随机，Forks >> Stars |
| b-nnett/goose | 已归档（archived=True） |
| rpanigrahi222/intruth-factcheck | 无描述 |
| SakanaAI/fugu | 无描述、无许可证 |
| world-action-models/awesome-* | 无描述 |
| Tyxy-R/codex-referral-risk-research | 无描述 |
| MSNightmare/RoguePlanet | 漏洞攻击工具，Forks/Stars 异常 |

---

## 五、推荐优先级总表

| 优先级 | 项目 | 推荐指数 | 核心亮点 |
|--------|------|----------|----------|
| 1 | ponytail | 9.5/10 | 影响最广，社区最热，量化效果显著 |
| 2 | omnigent | 9.0/10 | Agent 编排核心痛点，架构前瞻 |
| 3 | shadcn/improve | 8.5/10 | 模型级联创新，shadcn 品牌 |
| 4 | vercel/eve | 8.5/10 | Vercel 官方，文件系统优先设计 |
| 5 | baidu/Unlimited-OCR | 8.0/10 | OCR 技术突破，论文支撑 |

---

*报告结束*
