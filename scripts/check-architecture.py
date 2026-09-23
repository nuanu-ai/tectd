#!/usr/bin/env python3
"""Enforce the product's inward dependencies and bounded source files."""
from pathlib import Path
import hashlib
import json
import re
import sys
import tomllib

ROOT = Path(__file__).resolve().parents[1]
SOURCE_FILE_LIMIT = 500
BASELINE_PATH = ROOT / "scripts/architecture-file-baseline.json"
baseline = json.loads(BASELINE_PATH.read_text())
if baseline.get("schema_version") != 1 or not isinstance(baseline.get("files"), dict):
    raise SystemExit("invalid architecture file baseline")
frozen_files = baseline["files"]
for name, expected in frozen_files.items():
    if (not isinstance(name, str) or not name.startswith("crates/")
            or Path(name).is_absolute() or ".." in Path(name).parts
            or not isinstance(expected, dict)
            or set(expected) != {"lines", "sha256"}
            or not isinstance(expected["lines"], int)
            or expected["lines"] <= SOURCE_FILE_LIMIT
            or not isinstance(expected["sha256"], str)
            or not re.fullmatch(r"[0-9a-f]{64}", expected["sha256"])):
        raise SystemExit(f"invalid architecture baseline entry: {name}")
ALLOWED = {
    "tect-domain": {"serde", "serde_json", "uuid", "oxrdf"},
    "tect-application": {"tect-domain", "async-trait", "uuid", "sha2", "serde_json"},
    "tect-postgres": {
        "tect-domain", "tect-application", "async-trait", "sqlx", "uuid",
        "sha2", "getrandom", "serde", "serde_json", "oxrdf",
    },
    "tect-host": {
        "tect-domain", "tect-application", "serde", "serde_json", "tokio",
        "uuid", "async-trait", "rustix", "sha2",
    },
    "tect-cli": {
        "tect-domain", "tect-application", "tect-postgres", "tect-host",
        "tokio", "serde", "serde_json", "clap", "uuid", "rustix", "sha2", "url",
        "percent-encoding",
    },
}
errors = []
observed = set()
for manifest in sorted((ROOT / "crates").glob("*/Cargo.toml")):
    data = tomllib.loads(manifest.read_text())
    name = data["package"]["name"]
    observed.add(name)
    dependencies = data.get("dependencies", {})
    packages = {
        value.get("package", key) if isinstance(value, dict) else key
        for key, value in dependencies.items()
    }
    forbidden = packages - ALLOWED.get(name, set())
    if forbidden:
        errors.append(f"{name}: forbidden dependencies {sorted(forbidden)}")
    if data.get("build-dependencies") or data.get("target"):
        errors.append(f"{name}: unreviewed build/target dependency escape")
    if name in {"tect-domain", "tect-application"}:
        for source in manifest.parent.rglob("*.rs"):
            text = source.read_text()
            if re.search(r"\bstd\s*::\s*(fs|env|process|net|io)\b", text):
                errors.append(f"{source.relative_to(ROOT)}: outward standard-library API")
            for imported in re.findall(r"use\s+std\s*::\s*\{([^;]+)\}\s*;", text):
                if re.search(r"\b(fs|env|process|net|io)\b", imported):
                    errors.append(f"{source.relative_to(ROOT)}: outward grouped std import")
if observed != set(ALLOWED):
    errors.append(f"Unexpected crate set: {sorted(observed)}")
seen_frozen = set()
for source in sorted((ROOT / "crates").rglob("*")):
    if source.is_file() and source.suffix in {".rs", ".sql"}:
        relative = source.relative_to(ROOT).as_posix()
        content = source.read_bytes()
        lines = len(content.splitlines())
        if relative in frozen_files:
            seen_frozen.add(relative)
            expected = frozen_files[relative]
            if lines != expected["lines"] or hashlib.sha256(content).hexdigest() != expected["sha256"]:
                errors.append(f"{relative}: frozen legacy file changed; split it below {SOURCE_FILE_LIMIT} lines and remove its baseline entry")
        elif lines > SOURCE_FILE_LIMIT:
            errors.append(f"{relative}: {lines} lines, limit {SOURCE_FILE_LIMIT}")
for missing in sorted(frozen_files.keys() - seen_frozen):
    errors.append(f"{missing}: stale architecture baseline entry")
if errors:
    print("\n".join(errors), file=sys.stderr)
    raise SystemExit(1)
print(f"Architecture boundaries and source-file limits: PASS ({len(seen_frozen)} unchanged legacy files frozen)")
