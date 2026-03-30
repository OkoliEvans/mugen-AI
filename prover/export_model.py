"""
export_model.py
---------------
Exports a PyTorch model to ONNX format and generates a sample input.json
for EZKL witness generation.

Usage:
    python export_model.py [--model resnet18] [--out-dir ./artifacts]
"""

import argparse
import json
import os
import torch
import torch.nn as nn
import torchvision.models as models


# Tiny MLP — use this for Phase 1 / local dev
# 4 inputs → 16 hidden → 8 hidden → 2 outputs
# Proves in ~5s, RAM stays under 1GB, pk.key ~200MB
class TinyMLP(nn.Module):
    def __init__(self):
        super().__init__()
        self.net = nn.Sequential(
            nn.Linear(4, 16),
            nn.ReLU(),
            nn.Linear(16, 8),
            nn.ReLU(),
            nn.Linear(8, 2),
        )

    def forward(self, x):
        return self.net(x)


SUPPORTED_MODELS = {
    # Phase 1 / local dev — fast, low RAM
    "tiny_mlp":     (TinyMLP,             (1, 4)),
    # Phase 2+ — production scale, needs beefy hardware
    "resnet18":     (models.resnet18,     (1, 3, 224, 224)),
    "resnet50":     (models.resnet50,     (1, 3, 224, 224)),
    "mobilenet_v2": (models.mobilenet_v2, (1, 3, 224, 224)),
}


def export_model(model_name: str, out_dir: str) -> None:
    os.makedirs(out_dir, exist_ok=True)

    if model_name not in SUPPORTED_MODELS:
        raise ValueError(
            f"Unsupported model '{model_name}'. Choose from: {list(SUPPORTED_MODELS)}"
        )

    model_fn, input_shape = SUPPORTED_MODELS[model_name]

    # TinyMLP is instantiated directly, torchvision models need weights=None
    if model_name == "tiny_mlp":
        model = model_fn()
    else:
        model = model_fn(weights=None)

    model.eval()
    dummy_input = torch.randn(*input_shape)

    onnx_path = os.path.join(out_dir, "model.onnx")
    torch.onnx.export(
        model,
        dummy_input,
        onnx_path,
        opset_version=11,
        input_names=["input"],
        output_names=["output"],
    )
    print(f"[export] model: {model_name}")
    print(f"[export] model.onnx written to {onnx_path}")

    input_json_path = os.path.join(out_dir, "input.json")
    sample = {"input_data": [dummy_input.flatten().tolist()]}
    with open(input_json_path, "w") as f:
        json.dump(sample, f)
    print(f"[export] input.json written to {input_json_path}")

    if model_name == "tiny_mlp":
        print("\n[export] NOTE: tiny_mlp selected — optimised for Phase 1 local proving")
        print("         swap to resnet18/mobilenet_v2 for production scale")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description="Export PyTorch model to ONNX")
    parser.add_argument(
        "--model",
        default="tiny_mlp",
        choices=SUPPORTED_MODELS.keys(),
        help="Model to export. Use tiny_mlp for Phase 1 local dev (default)",
    )
    parser.add_argument("--out-dir", default="./artifacts")
    args = parser.parse_args()

    export_model(args.model, args.out_dir)