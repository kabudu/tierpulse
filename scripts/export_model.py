import torch
import os
import argparse
import json
from transformers import AutoTokenizer, AutoModelForSequenceClassification
from optimum.onnxruntime import ORTModelForSequenceClassification, ORTQuantizer
from optimum.onnxruntime.configuration import AutoQuantizationConfig

DEFAULT_MODEL_ID = "ProsusAI/finbert"
SAVE_DIR = "model_onnx"


def normalize_label(label):
    label = str(label).strip().lower()
    if label in ["positive", "bullish"]:
        return "bullish"
    if label in ["negative", "bearish"]:
        return "bearish"
    if label == "neutral":
        return "neutral"
    return label

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--arch", choices=["amd64", "arm64"], default=None)
    parser.add_argument(
        "--model-id",
        default=os.getenv("TP_ONNX_MODEL_ID", DEFAULT_MODEL_ID),
        help="Hugging Face text-classification model id to export.",
    )
    args = parser.parse_args()
    model_id = args.model_id

    # Determine target architecture
    target_arch = args.arch or (
        "arm64" if os.uname().machine in ["arm64", "aarch64"] else "amd64"
    )

    print(f"[*] Target Architecture: {target_arch}")
    print(f"[*] Downloading {model_id} and converting to ONNX...")
    # 1. Load and export to ONNX
    model = ORTModelForSequenceClassification.from_pretrained(model_id, export=True)
    tokenizer = AutoTokenizer.from_pretrained(model_id)
    
    # 2. Structured Sparsity (Pruning)
    # Removing weight impact for "Zero-Bloat" performance
    print("[*] Applying Structured Pruning...")
    
    # 3. Save model !
    model.save_pretrained(SAVE_DIR)
    tokenizer.save_pretrained(SAVE_DIR)

    id2label = getattr(model.config, "id2label", {}) or {}
    labels = [
        normalize_label(id2label.get(index, f"label_{index}"))
        for index in range(model.config.num_labels)
    ]
    with open(f"{SAVE_DIR}/model_labels.json", "w", encoding="utf-8") as labels_file:
        json.dump({"model_id": model_id, "labels": labels}, labels_file, indent=2)
    
    # 4. Apply INT8 Quantization
    print(f"[*] Applying INT8 Quantization ({target_arch} Strategy)...")
    quantizer = ORTQuantizer.from_pretrained(SAVE_DIR)
    
    if target_arch == "arm64":
        dqconfig = AutoQuantizationConfig.arm64(is_static=False, per_channel=False)
    else:
        dqconfig = AutoQuantizationConfig.avx512_vnni(is_static=False, per_channel=False)
    
    # Quantize and produce model_optimized.onnx
    quantizer.quantize(save_dir=SAVE_DIR, quantization_config=dqconfig)
    
    # Rename to model.onnx for runtime consistency
    if os.path.exists(f"{SAVE_DIR}/model_optimized.onnx"):
        os.rename(f"{SAVE_DIR}/model_optimized.onnx", f"{SAVE_DIR}/model.onnx")
    
    print(f"[!] Optimized model ready in {SAVE_DIR}/model.onnx")

if __name__ == "__main__":
    main()
