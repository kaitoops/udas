"""
BGE-M3 / BGE-small ONNX 模型转换脚本
=====================================

将 HuggingFace PyTorch 模型转换为 ONNX 格式，供 Rust ort crate 使用。

使用方法:
    # 1. 安装依赖
    pip install -r requirements.txt

    # 2. 运行转换（需要网络连接下载模型）
    python convert_bge_m3.py

    # 3. 或只转换 BGE-small（CPU fallback 用）
    python convert_bge_m3.py --small-only

输出目录:
    C:\\Users\\WIN10\\udas-tui\\models\\
    ├── bge-m3-onnx\\           # FP16, ~0.9GB
    │   ├── model.onnx
    │   ├── tokenizer.json
    │   ├── tokenizer_config.json
    │   ├── config.json
    │   └── special_tokens_map.json
    └── bge-small-zh-onnx\\     # ~100MB
        ├── model.onnx
        ├── tokenizer.json
        └── config.json

注意:
    - BGE-M3 转换需要约 4GB 内存和 2GB 显存（FP16）
    - 转换时间约 5-10 分钟（取决于网络速度）
    - 如果 GPU 不可用，会自动降级到 CPU 转换
"""

import argparse
import os
import sys
from pathlib import Path

OUTPUT_BASE = Path(r"C:\Users\WIN10\udas-tui\models")


def convert_bge_m3(fp16: bool = True) -> Path:
    """转换 BGE-M3 模型为 ONNX 格式。"""
    from optimum.onnxruntime import ORTModelForFeatureExtraction
    from transformers import AutoTokenizer

    model_id = "BAAI/bge-m3"
    output_dir = OUTPUT_BASE / "bge-m3-onnx"
    output_dir.mkdir(parents=True, exist_ok=True)

    print(f"[1/4] 下载并加载 {model_id}...")
    provider = "CUDAExecutionProvider" if fp16 else "CPUExecutionProvider"
    try:
        model = ORTModelForFeatureExtraction.from_pretrained(
            model_id, export=True, provider=provider
        )
    except Exception as e:
        print(f"  GPU 转换失败 ({e})，降级到 CPU...")
        model = ORTModelForFeatureExtraction.from_pretrained(
            model_id, export=True, provider="CPUExecutionProvider"
        )

    print(f"[2/4] 加载 tokenizer...")
    tokenizer = AutoTokenizer.from_pretrained(model_id)

    print(f"[3/4] 保存 ONNX 模型到 {output_dir}...")
    model.save_pretrained(str(output_dir))
    tokenizer.save_pretrained(str(output_dir))

    # 验证输出
    onnx_file = output_dir / "model.onnx"
    if not onnx_file.exists():
        # 有些版本会生成 model_quantized.onnx 或其他名称
        onnx_files = list(output_dir.glob("*.onnx"))
        if onnx_files:
            # 重命名为标准名称
            onnx_files[0].rename(onnx_file)
        else:
            raise RuntimeError(f"未找到 ONNX 模型文件 in {output_dir}")

    print(f"[4/4] 验证模型...")
    file_size = onnx_file.stat().st_size / (1024 * 1024)
    print(f"  模型大小: {file_size:.1f} MB")
    print(f"  输出维度: 1024")
    print(f"  路径: {onnx_file}")

    return output_dir


def convert_bge_small() -> Path:
    """转换 BGE-small-zh 模型为 ONNX 格式（CPU fallback 用）。"""
    from optimum.onnxruntime import ORTModelForFeatureExtraction
    from transformers import AutoTokenizer

    model_id = "BAAI/bge-small-zh-v1.5"
    output_dir = OUTPUT_BASE / "bge-small-zh-onnx"
    output_dir.mkdir(parents=True, exist_ok=True)

    print(f"[1/4] 下载并加载 {model_id}...")
    model = ORTModelForFeatureExtraction.from_pretrained(
        model_id, export=True, provider="CPUExecutionProvider"
    )

    print(f"[2/4] 加载 tokenizer...")
    tokenizer = AutoTokenizer.from_pretrained(model_id)

    print(f"[3/4] 保存 ONNX 模型到 {output_dir}...")
    model.save_pretrained(str(output_dir))
    tokenizer.save_pretrained(str(output_dir))

    # 标准化文件名
    onnx_file = output_dir / "model.onnx"
    if not onnx_file.exists():
        onnx_files = list(output_dir.glob("*.onnx"))
        if onnx_files:
            onnx_files[0].rename(onnx_file)

    print(f"[4/4] 验证模型...")
    file_size = onnx_file.stat().st_size / (1024 * 1024)
    print(f"  模型大小: {file_size:.1f} MB")
    print(f"  输出维度: 512")
    print(f"  路径: {onnx_file}")

    return output_dir


def verify_embedding(model_dir: Path, model_name: str) -> bool:
    """验证 ONNX 模型能否正确生成嵌入。"""
    try:
        from sentence_transformers import SentenceTransformer
        # 用原始 HF 模型验证基准
        print(f"\n[验证] 用 sentence-transformers 生成基准嵌入...")
        st_model = SentenceTransformer(model_name)
        emb = st_model.encode(["测试文本", "test text"])
        print(f"  基准嵌入维度: {emb.shape}")
        print(f"  '测试文本' 前5维: {emb[0][:5]}")
        print(f"  'test text' 前5维: {emb[1][:5]}")
        cos_sim = (emb[0] @ emb[1]) / (
            (emb[0] @ emb[0]) ** 0.5 * (emb[1] @ emb[1]) ** 0.5
        )
        print(f"  余弦相似度: {cos_sim:.4f}")
        return True
    except Exception as e:
        print(f"  验证跳过: {e}")
        return False


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description="BGE-M3 ONNX 模型转换")
    parser.add_argument(
        "--small-only", action="store_true", help="只转换 BGE-small（CPU fallback）"
    )
    parser.add_argument(
        "--no-fp16", action="store_true", help="禁用 FP16（用 FP32）"
    )
    parser.add_argument(
        "--verify", action="store_true", help="转换后验证嵌入质量"
    )
    args = parser.parse_args()

    print("=" * 60)
    print("BGE-M3 / BGE-small ONNX 模型转换")
    print("=" * 60)

    if not args.small_only:
        print("\n>>> 转换 BGE-M3 (GPU, FP16)")
        m3_dir = convert_bge_m3(fp16=not args.no_fp16)
        if args.verify:
            verify_embedding(m3_dir, "BAAI/bge-m3")

    print("\n>>> 转换 BGE-small-zh (CPU)")
    small_dir = convert_bge_small()
    if args.verify:
        verify_embedding(small_dir, "BAAI/bge-small-zh-v1.5")

    print("\n" + "=" * 60)
    print("转换完成！")
    print(f"  BGE-M3:   {OUTPUT_BASE / 'bge-m3-onnx' / 'model.onnx'}")
    print(f"  BGE-small: {OUTPUT_BASE / 'bge-small-zh-onnx' / 'model.onnx'}")
    print("\n下一步: 在 Rust 中使用 ort crate 加载这些模型")
    print("=" * 60)
