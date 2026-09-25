#!/usr/bin/env python3
"""Enforce the product's inward dependencies and bounded source files."""
from pathlib import Path
import re
import sys
import tomllib
from architecture_sources import over_limit_sources, test_only_sources

ROOT = Path(__file__).resolve().parents[1]
ALLOWED = {
    # SHA-256 computes deterministic value identity; it performs no effect or I/O.
    "tect-domain": {"serde", "serde_json", "sha2", "uuid", "oxrdf"},
    "tect-application": {"tect-domain", "async-trait", "uuid", "sha2", "serde_json"},
    "tect-postgres": {
        "tect-domain", "tect-application", "async-trait", "sqlx", "uuid",
        "sha2", "getrandom", "serde", "serde_json", "oxrdf",
    },
    "tect-host": {
        "tect-domain", "tect-application", "serde", "serde_json", "tokio",
        "uuid", "async-trait", "rustix", "sha2", "reqwest",
    },
    "tect-cli": {
        "tect-domain", "tect-application", "tect-postgres", "tect-host",
        "tokio", "serde", "serde_json", "clap", "uuid", "rustix", "sha2", "url",
        "percent-encoding",
    },
}
errors = []
observed = set()


test_only = test_only_sources(ROOT)
domain_advisory_sources = [ROOT / "crates/domain/src/advisory.rs"]
domain_advisory_sources.extend(
    source
    for source in (ROOT / "crates/domain/src/advisory").rglob("*.rs")
    if source.resolve() not in test_only
)
for source in sorted(domain_advisory_sources):
    text = source.read_text()
    for forbidden in (
        "sqlx::", "std::env", "reqwest::", "hyper::", "tect_postgres", "tect_host",
    ):
        if forbidden in text:
            errors.append(
                f"{source.relative_to(ROOT)}: forbidden advisory dependency {forbidden}"
            )

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
for source, lines in over_limit_sources(ROOT):
    errors.append(f"{source.relative_to(ROOT)}: {lines} lines, limit 500")
if errors:
    print("\n".join(errors), file=sys.stderr)
    raise SystemExit(1)
print("Architecture boundaries and source-file limits: PASS")
