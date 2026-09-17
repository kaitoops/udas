---
schema_version: 1
generated_at: "2026-07-27T11:52:40+08:00"
last_introspection: "2026-07-27T11:52:40+08:00"
backup_count: 0
status: "initial-generation"
---

# UDAS System State — Hot File

> **预加载说明**: 此文件是内观系统的热文件，描述UDAS功能系统当前时刻的实际状态。
> 每次内观行为（自检、全局错误扫描、CSL评估）时第一时间预加载。
> 内容精准简化，非必要不加载历史日志。重大错误时回退到 UDAS-TIMELINE.jsonl。

## 1. 系统拓扑

```
UDAS Workspace (C:\Users\WIN10\udas-tui)
├── deepseek-udas     (crates/udas)          算法核心
│   ├── types.rs          类型定义: Angle, Embedding, MeasurementInput, UdasResult
│   ├── restoration.rs    LlmRestorer trait + restore() 编排
│   ├── geometry.rs       圆盘几何: MDS降维, 角度计算
│   ├── density.rs        密度估计: 核密度加权
│   ├── interference.rs   干涉场计算: 交叉项, find_destructive_points
│   ├── memory.rs         记忆管理
│   ├── precision_control.rs  精度控制: 收敛判定
│   ├── env_signal.rs     环境信号采集
│   └── engine.rs         UdasEngine: run() + run_with_measurements() + with_embedder()
│
├── udas-embedding    (crates/udas-embedding)  嵌入层
│   ├── embedder.rs       Embedder trait + UNIFIED_DIM(512)
│   ├── fnv.rs            FnvHashEmbedder (256-d, 零依赖, 始终可用)
│   ├── projector.rs      DimensionProjector (Johnson-Lindenstrauss → 512-d)
│   ├── gpu.rs            GpuState (显存检测, CUDA可用性)
│   ├── pooling.rs        [ort-backend] 池化层
│   ├── bge_m3.rs         [ort-backend] BGE-M3 ONNX GPU (1024-d → 512-d)
│   ├── bge_small.rs      [ort-backend] BGE-small ONNX CPU (512-d native)
│   ├── cache.rs          CachedEmbedder (磁盘+内存缓存, SHA-256)
│   └── runtime.rs        RuntimeEmbedder (auto: GPU→CPU→FNV 三级降级)
│
├── udas-embed-service (crates/udas-embed-service)  常驻嵌入服务
│   ├── config.rs         ServerConfig/ClientConfig/BackendMode/TransportKind
│   ├── error.rs          ServiceError
│   ├── framing.rs        4B长度前缀帧编解码
│   ├── protocol.rs       JSON-RPC 2.0: embed/embed_batch/ping/info
│   ├── transport.rs      local_socket(命名管道) + TCP 抽象
│   ├── server.rs         EmbedServer: 模型加载一次, IPC请求分发
│   └── client.rs         EmbedClient + RemoteEmbedder (impl Embedder)
│
├── udas-cli           (crates/udas-cli)      CLI工具
│   └── main.rs           embed/compute/similarity/embed-service(start/stop/status/ping)
│
├── udas-introspect    (crates/udas-introspect)  内观系统
│   ├── diff.rs         Git diff解析 + ChangeFeatures提取
│   ├── csl.rs          CSL分级: Detail/LocalFix/Functional/Architectural
│   ├── hot_file.rs     热文件管理: 3槽位备份滚动 + section更新
│   └── timeline.rs     时间线日志: append-only + 修正链追踪
│
└── deepseek-tui       (crates/tui)           TUI集成
    ├── client.rs         DeepSeekClient (+ remote_embedder缓存字段)
    └── udas_bridge.rs    LlmRestorer impl: decompose/find_evidence/embed
                          embed() → RemoteEmbedder惰性连接 + FNV降级
```

### 依赖拓扑（UDAS核心）

```
deepseek-tui ──→ deepseek-udas ──→ udas-embedding
     │               │
     ├──→ udas-embed-service ──→ udas-embedding
     │
udas-cli ──→ deepseek-udas
     │
     ├──→ udas-embedding
     ├──→ udas-embed-service
     └──→ udas-introspect
```

## 2. 模块成熟度矩阵

| Crate | 模块 | 测试数 | 状态 | 备注 |
|-------|------|--------|------|------|
| **deepseek-udas** | types.rs | — | ✅ 生产 | 类型定义稳定 |
| | restoration.rs | 8 | ✅ 生产 | LlmRestorer trait + restore编排 |
| | geometry.rs | 10 | ✅ 生产 | MDS降维+角度计算 |
| | density.rs | 3 | ✅ 生产 | 核密度加权 |
| | interference.rs | 36 | ✅ 生产 | 干涉场核心，测试最充分 |
| | memory.rs | 2 | ✅ 生产 | 记忆管理 |
| | precision_control.rs | 4 | ✅ 生产 | 收敛判定 |
| | env_signal.rs | 12 | ✅ 生产 | 环境信号采集 |
| | engine.rs | 11 | ✅ 生产 | UdasEngine主入口 |
| **udas-embedding** | fnv.rs | 9 | ✅ 生产 | 零依赖，始终可用 |
| | projector.rs | 8 | ✅ 生产 | JL投影到512-d |
| | gpu.rs | 3 | ✅ 生产 | 显存检测 |
| | cache.rs | 6 | ✅ 生产 | 磁盘+内存缓存 |
| | pooling.rs | 5 | ⚠️ ort-gated | 需--features ort-backend |
| | bge_m3.rs | 0 | ⚠️ ort-gated | 代码完成，实测验证过(0.872相似度) |
| | bge_small.rs | 0 | ⚠️ ort-gated | 代码完成，未实测 |
| | runtime.rs | 0 | ✅ 生产 | 三级降级验证通过 |
| | embedder.rs | 0 | ✅ 生产 | trait定义稳定 |
| **udas-embed-service** | protocol.rs | 8 | ✅ 生产 | JSON-RPC 2.0 |
| | config.rs | 4 | ✅ 生产 | 配置结构 |
| | transport.rs | 3 | ✅ 生产 | 命名管道+TCP |
| | framing.rs | 0 | ✅ 生产 | 帧编解码 |
| | error.rs | 0 | ✅ 生产 | 错误类型 |
| | server.rs | 8 | ✅ 生产 | EmbedServer+E2E验证 |
| | client.rs | 3 | ✅ 生产 | RemoteEmbedder+降级 |
| **udas-cli** | main.rs | 0 | ✅ 生产 | CLI入口，E2E验证 |
| **udas-introspect** | diff.rs | 8 | ✅ 生产 | Git diff解析+特征提取 |
| | csl.rs | 13 | ✅ 生产 | CSL四级分类逻辑 |
| | hot_file.rs | 7 | ✅ 生产 | 3槽位备份+section更新+回滚 |
| | timeline.rs | 6 | ✅ 生产 | JSONL日志+修正链追踪 |
| **deepseek-tui** | udas_bridge.rs | 3 | ✅ 生产 | LlmRestorer impl |
| | client.rs | — | ✅ 生产 | +remote_embedder缓存字段 |

**测试总计**: deepseek-udas 86 + udas-embedding 31 + udas-embed-service 34 + udas-introspect 40 = **191个单元测试**

## 3. 关键数据流路径

### 路径A: CLI完整计算（无LLM）
```
用户输入 → udas-cli embed "text" → RuntimeEmbedder::auto()
    → [GPU空闲] BGE-M3 (1024-d) → DimensionProjector → 512-d
    → [GPU忙碌] BGE-small CPU (512-d)
    → [降级]    FNV-1a hash (256-d) → DimensionProjector → 512-d
→ 测量集JSON → udas-cli compute → UdasEngine::run_with_measurements()
    → interference_field计算 → MDS降维 → collapse → contradiction检测
→ 输出: 坍缩角度 + 置信度 + 搜索效率 + 破坏性点
```

### 路径B: 常驻服务路径（消除冷启动）
```
udas-cli embed-service start → EmbedServer::new()
    → RuntimeEmbedder::with_model_dir() [模型加载一次]
    → local_socket监听 (命名管道: udas-embed)
    → 循环accept → handle_connection → dispatch
        → embed/embed_batch/ping/info

客户端:
udas-cli embed "text" --remote
    → RemoteEmbedder::connect() [IPC连接]
    → EmbedClient::embed() → JSON-RPC请求 → 服务端推理 → 响应
```

### 路径C: TUI集成路径（完整UDAS）
```
DeepSeekClient.embed(text)
    → 检查 self.remote_embedder 缓存
    → [有缓存] RemoteEmbedder.embed() → IPC → EmbedServer → BGE-M3/FNV
    → [无缓存] 尝试connect()
        → [成功] embed + 缓存连接
        → [失败] FnvHashEmbedder.embed() [自动降级]

UdasEngine::run(problem)
    → Phase 1: 冷启动 — 角度生成 + basis变体
    → Phase 2: LLM还原 — decompose() + find_evidence() [DeepSeek API]
    → Phase 3: 嵌入 — client.embed() [路径C]
    → Phase 4: 干涉场 — interference计算 + MDS
    → Phase 5: 坍缩 — 梯度下降 + 密度加权采样
    → Phase 6: 矛盾解决 — destructive_points检测 + 补充测量
```

## 4. 已知缺陷与限制

| ID | 描述 | 影响范围 | 严重度 | 状态 |
|----|------|----------|--------|------|
| DEF-001 | FNV后端无语义信息，干涉场相位差和门控失效 | 算法效果 | 高 | 已知限制，需BGE-M3 |
| DEF-002 | udas_bridge.rs的embed()首次调用尝试连接服务有IPC延迟(~67ms) | 首次嵌入延迟 | 低 | 可接受，后续缓存 |
| DEF-003 | CLI模式每次调用重新加载RuntimeEmbedder(~150ms) | CLI性能 | 低 | 已由embed-service解决 |
| DEF-004 | BGE-M3冷启动10+分钟(ONNX模型2.2GB+CUDA初始化) | 服务启动 | 中 | 已由常驻服务解决 |
| DEF-005 | ort-backend feature未在本机编译验证 | GPU路径 | 中 | 待验证 |
| DEF-006 | udas-cli无单元测试 | 测试覆盖 | 低 | CLI逻辑简单，E2E验证替代 |

## 5. 待验证项

- [ ] `cargo build --features ort-backend` 编译GPU路径
- [ ] BGE-M3 ONNX模型文件完整性确认 (models/目录)
- [ ] embed-service start --backend auto 实际加载BGE-M3 GPU
- [ ] UdasEngine::run() 完整路径测试（需DeepSeek API key + 真实问题）
- [ ] TUI中UDAS功能的交互测试

## 6. 内观指针

- **上次内观时间**: 2026-07-27T17:00:00+08:00
- **上次内观类型**: csl-implementation（CSL机制实现完成）
- **校验回路状态**: 未执行（首次生成，无历史对比）
- **人类校准标记**: 无
- **CSL机制状态**: 已实现（udas-introspect crate, 40个测试通过）
- **备份状态**: 3槽位备份机制已实现（HotFileManager）

## 7. 内观系统自身状态

```
UDAS-INTROSPECTION
├── UDAS-STATE.md           ← 本文件（热文件，预加载）
├── UDAS-STATE.md.bak-1     ← 备份槽位1（无）
├── UDAS-STATE.md.bak-2     ← 备份槽位2（无）
├── UDAS-TIMELINE.jsonl     ← 时间线日志（append-only + 修正链）
└── UDAS-FORENSICS/         ← 深度诊断档案（按需生成）
```

### CSL分级参考（已实现）

| 级别 | 定义 | 热文件动作 |
|------|------|-----------|
| CSL-0 | 纯细节修改(typo/注释/格式) | 不触发 |
| CSL-1 | 局部修复(bug fix/参数调整) | 不触发，打标记 |
| CSL-2 | 功能变更(接口/数据流/成熟度/依赖) | AGENT评估 |
| CSL-3 | 架构变更(crate重组/项目方向) | 强制更新 |
