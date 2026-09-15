#!/usr/bin/env python3
"""Check declared engineering-review file evidence against one repository root."""

from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
import sys


KINDS = {"behavioral", "mixed", "declarative"}
MAX_FILES = 100
MAX_FILE_BYTES = 16 * 1024 * 1024
CHUNK_BYTES = 64 * 1024


def physical_lines(data: bytes) -> int:
    if not data:
        return 0
    return data.count(b"\n") + (0 if data.endswith(b"\n") else 1)


def inspect_file(path: Path) -> tuple[int, str]:
    size = 0
    newlines = 0
    last = b""
    content_digest = hashlib.sha256()
    with path.open("rb") as source:
        while chunk := source.read(CHUNK_BYTES):
            size += len(chunk)
            if size > MAX_FILE_BYTES:
                raise ValueError(f"{path.name}: file exceeds {MAX_FILE_BYTES} bytes")
            content_digest.update(chunk)
            newlines += chunk.count(b"\n")
            if newlines > 1500:
                raise ValueError(f"{path.name}: line count exceeds 1500")
            last = chunk[-1:]
    lines = newlines + (1 if size and last != b"\n" else 0)
    return lines, content_digest.hexdigest()


def safe_file(root: Path, relative: str) -> Path:
    supplied = Path(relative)
    if supplied.is_absolute() or "\\" in relative or "\0" in relative:
        raise ValueError(f"unsafe file path: {relative}")
    parts = supplied.parts
    if not parts or any(part in {"", ".", ".."} for part in parts):
        raise ValueError(f"unsafe file path: {relative}")
    current = root
    for part in parts:
        current = current / part
        if current.is_symlink():
            raise ValueError(f"symlink path is not allowed: {relative}")
    candidate = current.resolve(strict=True)
    if candidate == root or root not in candidate.parents or not candidate.is_file():
        raise ValueError(f"file escapes repository root or is missing: {relative}")
    return candidate


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--repo-root", required=True)
    parser.add_argument("--manifest", required=True)
    args = parser.parse_args()
    root = Path(args.repo_root).resolve(strict=True)
    manifest_path = Path(args.manifest).resolve(strict=True)
    manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    entries = manifest.get("files")
    if not isinstance(entries, list):
        raise ValueError("manifest.files must be an array")
    if len(entries) > MAX_FILES:
        raise ValueError(f"manifest.files exceeds {MAX_FILES} entries")
    seen: set[str] = set()
    checked = []
    for entry in entries:
        if not isinstance(entry, dict):
            raise ValueError("each manifest file must be an object")
        relative = entry.get("path")
        kind = entry.get("content_kind")
        justification = entry.get("justification")
        responsibility = entry.get("responsibility")
        if not isinstance(relative, str) or not relative or relative in seen:
            raise ValueError("file paths must be nonempty and unique")
        if kind not in KINDS:
            raise ValueError(f"invalid content_kind for {relative}")
        if not isinstance(responsibility, str) or not responsibility.strip():
            raise ValueError(f"responsibility must be a nonblank string for {relative}")
        if justification is not None and (
            not isinstance(justification, str) or not justification.strip()
        ):
            raise ValueError(f"justification must be a nonblank string for {relative}")
        seen.add(relative)
        candidate = safe_file(root, relative)
        lines, content_digest = inspect_file(candidate)
        if lines > 1500:
            raise ValueError(f"{relative}: {lines} lines exceeds 1500")
        if lines > 1000 and kind != "declarative":
            raise ValueError(f"{relative}: more than 1000 lines requires declarative content")
        if lines > 500 and (not isinstance(justification, str) or not justification.strip()):
            raise ValueError(f"{relative}: more than 500 lines requires justification")
        checked.append({
            "path": relative,
            "content_kind": kind,
            "line_count": lines,
            "count_basis": "observed",
            "content_digest": content_digest,
            "responsibility": responsibility,
            **({"justification": justification} if justification is not None else {}),
        })
    json.dump({"files": checked}, sys.stdout, indent=2)
    sys.stdout.write("\n")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, ValueError, json.JSONDecodeError) as error:
        print(f"check_files: {error}", file=sys.stderr)
        raise SystemExit(2)
