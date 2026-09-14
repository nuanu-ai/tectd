#!/usr/bin/env python3
"""Bounded offline multilingual-e5 embedding worker over JSON lines."""

import argparse
import contextlib
import hashlib
import json
import math
import os
import sys

PROTOCOL = "tect-knowledge-embedding-v1"
MODEL = "intfloat/multilingual-e5-small"
REVISION = "614241f622f53c4eeff9890bdc4f31cfecc418b3"
RECIPE = "title_v1"
DIMENSIONS = 384
MAX_TEXT_BYTES = 4 * 1024
MAX_REQUEST_BYTES = 8 * 1024
ASSETS = {
    "config.json": "69137736cab8b8903a07fe8afaafdda25aac55415a12a55d1bffa9f581abf959",
    "model.safetensors": "1a55775f53449dac10a2bcbc312469fac40b96d53198c407081a831f81c98477",
    "tokenizer.json": "0b44a9d7b51c3c62626640cda0e2c2f70fdacdc25bbbd68038369d14ebdf4c39",
    "tokenizer_config.json": "a1d6bc8734a6f635dc158508bef000f8e2e5a759c7d92f984b2c86e5ff53425b",
    "special_tokens_map.json": "d05497f1da52c5e09554c0cd874037a083e1dc1b9cfd48034d1c717f1afc07a7",
    "sentencepiece.bpe.model": "cfc8146abe2a0488e9e2a0c56de7952f7c11ab059eca145a0a727afce0db2865",
    "README.md": "0038de97aee16258cecbad7ffda4b4febd6953e747a00e0ddbc8e6ed241e9c1c",
}


def emit(value):
    sys.stdout.write(json.dumps(value, allow_nan=False, separators=(",", ":")) + "\n")
    sys.stdout.flush()


def fail(code):
    emit({"protocol": PROTOCOL, "status": "error", "code": code})


def request(value):
    if not isinstance(value, dict) or set(value) != {"protocol", "request_id", "kind", "text"}:
        raise ValueError("invalid_request")
    if value["protocol"] != PROTOCOL:
        raise ValueError("invalid_protocol")
    request_id = value["request_id"]
    kind = value["kind"]
    text = value["text"]
    if not isinstance(request_id, str) or not request_id or len(request_id) > 128:
        raise ValueError("invalid_request_id")
    if kind not in ("query", "passage"):
        raise ValueError("invalid_kind")
    if not isinstance(text, str) or not text.strip() or "\x00" in text:
        raise ValueError("invalid_text")
    if len(text.encode("utf-8")) > MAX_TEXT_BYTES:
        raise ValueError("text_too_large")
    return request_id, kind, text


def load(model_dir):
    os.environ["HF_HUB_OFFLINE"] = "1"
    os.environ["TRANSFORMERS_OFFLINE"] = "1"
    os.environ["TOKENIZERS_PARALLELISM"] = "false"
    for relative, expected in ASSETS.items():
        path = os.path.join(model_dir, relative)
        digest = hashlib.sha256()
        with open(path, "rb") as asset:
            for chunk in iter(lambda: asset.read(1024 * 1024), b""):
                digest.update(chunk)
        if digest.hexdigest() != expected:
            raise RuntimeError("invalid_asset")
    with open(os.devnull, "w", encoding="utf-8") as sink:
        with contextlib.redirect_stdout(sink), contextlib.redirect_stderr(sink):
            from transformers import AutoModel, AutoTokenizer, logging
            import torch

            logging.set_verbosity_error()
            torch.set_num_threads(4)
            torch.set_num_interop_threads(1)
            tokenizer = AutoTokenizer.from_pretrained(
                model_dir, local_files_only=True, trust_remote_code=False
            )
            model = AutoModel.from_pretrained(
                model_dir,
                local_files_only=True,
                trust_remote_code=False,
                use_safetensors=True,
            )
    model.to("cpu")
    model.eval()
    if int(model.config.hidden_size) != DIMENSIONS:
        raise RuntimeError("invalid_dimensions")
    return model, tokenizer


def embed(model, tokenizer, kind, text):
    import torch

    prefix = "query: " if kind == "query" else "passage: "
    if not text.startswith(prefix):
        raise ValueError("invalid_prefix")
    encoded = tokenizer(
        [text],
        max_length=512,
        padding=True,
        truncation=True,
        return_tensors="pt",
    )
    with torch.no_grad():
        output = model(**encoded).last_hidden_state
    mask = encoded["attention_mask"].unsqueeze(-1).expand(output.size()).float()
    pooled = torch.sum(output * mask, dim=1) / torch.clamp(mask.sum(dim=1), min=1e-9)
    vector = torch.nn.functional.normalize(pooled, p=2, dim=1)[0].tolist()
    if len(vector) != DIMENSIONS or not all(math.isfinite(item) for item in vector):
        raise RuntimeError("invalid_embedding")
    norm = math.sqrt(sum(item * item for item in vector))
    if abs(norm - 1.0) > 1e-3:
        raise RuntimeError("invalid_norm")
    return vector


def main():
    parser = argparse.ArgumentParser(add_help=False)
    parser.add_argument("--model-dir", required=True)
    args = parser.parse_args()
    if not os.path.isabs(args.model_dir) or not os.path.isdir(args.model_dir):
        return 2
    try:
        model, tokenizer = load(args.model_dir)
    except Exception:
        return 3
    emit(
        {
            "protocol": PROTOCOL,
            "status": "ready",
            "model": MODEL,
            "revision": REVISION,
            "recipe": RECIPE,
            "dimensions": DIMENSIONS,
        }
    )
    while True:
        raw = sys.stdin.buffer.readline(MAX_REQUEST_BYTES + 1)
        if not raw:
            break
        if len(raw) > MAX_REQUEST_BYTES or not raw.endswith(b"\n"):
            fail("request_too_large")
            return 4
        try:
            value = json.loads(raw)
            request_id, kind, text = request(value)
            vector = embed(model, tokenizer, kind, text)
            emit(
                {
                    "protocol": PROTOCOL,
                    "status": "ok",
                    "request_id": request_id,
                    "model": MODEL,
                    "revision": REVISION,
                    "recipe": RECIPE,
                    "dimensions": DIMENSIONS,
                    "embedding": vector,
                }
            )
        except json.JSONDecodeError:
            fail("invalid_request")
        except UnicodeError:
            fail("invalid_request")
        except ValueError as error:
            code = str(error)
            allowed = {
                "invalid_request",
                "invalid_protocol",
                "invalid_request_id",
                "invalid_kind",
                "invalid_text",
                "text_too_large",
                "invalid_prefix",
            }
            fail(code if code in allowed else "invalid_request")
        except Exception:
            fail("inference_failed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
