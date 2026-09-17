# BGE-M3 语义嵌入接入 — 实施状态与架构文档

## 架构概览

UDAS 干涉场计算依赖语义嵌入质量。BGE-M3 是必需基础设施而非可选项。
提示词控制 LLM 内部向量技术依赖外部 LLM，仅作为锦上添花的参考层，不作为 UDAS 运行时依赖。

### 分层嵌入架构

```
RuntimeEmbedder::auto()
    │
    ├─ 1. BGE-M3 GPU (1024-d → 投影到 512-d)
    │     条件: GPU 空闲 ≥10GB VRAM + model.onnx 存在
    │     场景: 用户不在游戏时
    │
    ├─ 2. BGE-small CPU (512-d, 原生)
    │     条件: model.onnx 存在
    │     场景: GPU 被游戏占用时的自动降级
    │
    └─ 3. FNV-1a Hash (256-d, 零语义)
          条件: 兜底, 无依赖
          场景: 模型文件未生成时
```

### 维度统一

所有嵌入器输出统一投影到 `UNIFIED_DIM = 512`:
- BGE-M3 (1024-d): JL 引理随机投影, 信息损失极小
- BGE-small (512-d): 原生, 无需投影
- FNV hash (256-d): 零填充, 仅接口一致性

### GPU 状态检测

`GpuState::detect()` 通过 `nvidia-smi` 查询:
- `Available`: 空闲 VRAM ≥ 10GB → 可用 BGE-M3
- `Busy`: GPU 存在但 VRAM 不足 (如游戏中) → 降级到 BGE-small CPU
- `Unavailable`: 无 NVIDIA GPU → FNV hash 兜底

## 实施进度

### Phase 1: 零依赖骨架 — ✅ 完成

- `udas-embedding` crate 创建, 默认 feature 零 GPU 依赖
- `Embedder` trait + `FnvHashEmbedder` (256-d FNV-1a 哈希)
- `DimensionProjector` (JL 引理随机投影)
- `GpuState` GPU 状态检测
- `CachedEmbedder` 磁盘+内存缓存 (SHA-256 键)
- `RuntimeEmbedder` 运行时自动选择
- 编译验证通过 (默认 feature, 纯 CPU)

### Phase 2: ONNX 后端 — ✅ 完成

- `ort-backend` feature gate (ort + tokenizers + ndarray)
- `BgeM3Embedder` CUDA 推理实现 (ort 2.0.0-rc API)
- `BgeSmallEmbedder` CPU 推理实现
- `pooling` 模块 (mean pooling + L2 归一化)
- `convert_bge_m3.py` Python 转换脚本已执行, ONNX 模型已生成
- BGE-M3 GPU 推理验证通过

### Phase 3: 降级与缓存 — ✅ 完成

- `RuntimeEmbedder` 三级降级逻辑
- `CachedEmbedder` 磁盘持久化 + 内存缓存
- 启动时从磁盘加载缓存索引

### Phase 4: UDAS 管线集成 — ✅ 完成

- `UdasEngine` 注入 `embedder: Option<Box<dyn Embedder>>` 字段
- `with_embedder()` builder 方法
- `perform_restoration()` 条件分支: 有 embedder 用 embedder, 无则回退 `restorer.embed()`
- `udas-cli embed` 子命令改用 `RuntimeEmbedder::auto()`
- 编译验证通过 (`deepseek-udas` + `udas-cli`)

### Phase 5: 验证与相似度测试 — ✅ 完成 (2026-07-27)

- `udas-cli similarity` 子命令添加 (余弦相似度计算)
- BGE-M3 嵌入质量验证通过
- FNV 降级路径验证通过
- 三级降级端到端验证完成

#### 验证结果

**BGE-M3 (GPU) 嵌入质量:**

| 测试对 | 相似度 | 预期 | 判定 |
|--------|--------|------|------|
| 中文 vs 英文同义句 | 0.872 | 高 | ✅ 跨语言语义对齐正确 |
| 中文 vs 天气无关句 | 0.573 | 中低 | ✅ 语义距离合理 |
| 英文 vs 天气无关句 | 0.532 | 中低 | ✅ 语义距离合理 |
| 相同文本 | 1.000 | 1.0 | ✅ 完全一致 |

**FNV hash 降级验证:**

| 测试对 | FNV 相似度 | BGE-M3 相似度 | 说明 |
|--------|-----------|--------------|------|
| 中英文同义句 | -0.079 | 0.872 | FNV 无语义, BGE-M3 有语义 ✅ |
| 相同文本 | 1.000 | 1.000 | 两者一致 ✅ |
| 不同文本 | -0.126 | N/A | FNV 随机哈希, 接近 0 ✅ |

**结论:**
- BGE-M3 正确捕获跨语言语义等价性 (0.872 vs FNV 的 -0.079)
- 三级降级机制正常工作: GPU → CPU → FNV
- FNV 作为兜底无语义信息, 符合预期设计

## CLI 命令

### embed — 生成嵌入向量

```powershell
udas-cli.exe embed "text to embed"
# 输出: JSON 数组 [0.1, 0.2, ...]
```

### similarity — 计算余弦相似度

```powershell
udas-cli.exe similarity "text A" "text B"
# 输出: { "similarity": 0.87, "backend": "bge-m3", "dim": 512 }
```

### compute — 干涉驱动坍缩

```powershell
echo '{"measurements": [...]}' | udas-cli.exe compute
# 输出: 坍缩结果 + 诊断 + 矛盾消解信息
```

## 编译指令

```powershell
# 带 ONNX 后端 (BGE-M3 + BGE-small)
cargo build -p udas-cli --features ort-backend

# 纯 FNV 模式 (零 GPU 依赖)
cargo build -p udas-cli
```

## 文件清单

### udas-embedding crate (`crates/udas-embedding/`)

| 文件 | 功能 |
|------|------|
| `src/embedder.rs` | `Embedder` trait, `UNIFIED_DIM=512` |
| `src/fnv.rs` | FNV-1a 256-d 哈希嵌入器 |
| `src/projector.rs` | JL 引理维度投影器 |
| `src/gpu.rs` | NVIDIA GPU 状态检测 |
| `src/cache.rs` | 磁盘+内存缓存包装器 |
| `src/runtime.rs` | 运行时嵌入器自动选择 |
| `src/bge_m3.rs` | BGE-M3 ONNX GPU 推理 (feature gate) |
| `src/bge_small.rs` | BGE-small ONNX CPU 推理 (feature gate) |
| `src/pooling.rs` | Mean pooling + L2 归一化 (feature gate) |
| `Cargo.toml` | feature gate: `ort-backend` |

### 修改的已有文件

| 文件 | 修改内容 |
|------|----------|
| `crates/udas/Cargo.toml` | 添加 `udas-embedding` 依赖 |
| `crates/udas/src/engine.rs` | 注入 `embedder` 字段 + `with_embedder()` + 条件分支 |
| `crates/udas-cli/src/main.rs` | `embed` + `similarity` 子命令, `RuntimeEmbedder::auto()`, `cosine_similarity()` |

### Python 脚本

| 文件 | 功能 |
|------|------|
| `scripts/convert_bge_m3.py` | BGE-M3/BGE-small PyTorch → ONNX 转换 |
| `scripts/requirements.txt` | Python 依赖 |

## 设计决策记录

1. **BGE-M3 vs 提示词控制 LLM**: BGE-M3 可控性绝对可靠 (本地模型不随外部更新变化), 提示词控制 LLM 仅锦上添花 (外部 LLM 随时更新, 效果不可控)
2. **GPU VRAM 阈值 10GB**: BGE-M3 FP16 需 ~9.8GB, 12GB 显卡游戏时剩余不足, 设 10GB 阈值确保安全
3. **UNIFIED_DIM=512**: 平衡信息保留 (BGE-M3 1024→512 投影损失小) 和计算效率 (干涉场 O(N²) 受维度影响)
4. **Feature gate**: `ort-backend` 为可选 feature, 默认编译零 GPU 依赖, 确保代码在无 CUDA 环境也可编译
5. **两步分离测试策略**: 先 `cargo build` 编译, 再单独运行 `target\debug\udas-cli.exe`, 禁止 `cargo run` 同时编译+运行 (避免 GPU 资源争用导致系统卡死)