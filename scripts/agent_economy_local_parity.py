#!/usr/bin/env python3
"""Deterministic, source-only v0.6/v0.7 parity and size smoke.

This intentionally measures definition/phase JSON only.  It never starts a
service, connects to a provider, or claims runtime/DB benchmark evidence.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
from typing import Any

REPO_ROOT = Path(__file__).resolve().parents[1]
WORKSPACE_ROOT = REPO_ROOT.parent.parent


SEMANTIC_CATEGORIES: dict[str, tuple[str, ...]] = {
    "request_fit": ("request", "fit"),
    "targets_boundaries": ("target", "boundar", "protected"),
    "authority": ("authority",),
    # v0.7 carries source refs directly; the legacy proxy also spells out
    # provenance.  The shared obligation is source/provenance traceability.
    "source_provenance": ("source",),
    "acceptance_proof_plan": ("acceptance", "proof"),
    "escalation_handoff": ("escalation", "handoff"),
    "review_findings": ("review", "finding"),
    "red_green_refactor": ("red", "green", "refactor"),
    "result_truth": ("result", "truth"),
    "deploy_promotion_impact": ("deploy", "promotion", "impact"),
}

P9_LOCAL_TARGETS = {
    "definition_bytes": 40 * 1024,
    "phase_bytes": 24 * 1024,
    "response_bytes": 50 * 1024,
    "frame_bytes": 8 * 1024 * 1024,
}


def compact_bytes(value: Any) -> int:
    return len(json.dumps(value, ensure_ascii=False, separators=(",", ":")).encode())


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def load_definition(path: Path) -> tuple[dict[str, Any], dict[str, Any]]:
    value = json.loads(path.read_text())
    if not isinstance(value, dict) or not isinstance(value.get("phases"), list):
        raise AssertionError(f"invalid definition shape: {path}")
    return value, {"file_bytes": path.stat().st_size, "compact_bytes": compact_bytes(value)}


def semantic_coverage(value: dict[str, Any]) -> dict[str, bool]:
    text = json.dumps(value, ensure_ascii=False, sort_keys=True).lower()
    return {name: all(term in text for term in terms) for name, terms in SEMANTIC_CATEGORIES.items()}


def definition_metrics(path: Path) -> dict[str, Any]:
    value, sizes = load_definition(path)
    phases = value["phases"]
    phase_sizes = {str(phase.get("id", i)): compact_bytes(phase) for i, phase in enumerate(phases)}
    required_fields = {
        str(phase.get("id", i)): len(phase.get("required_fields", []))
        for i, phase in enumerate(phases)
    }
    route_count = sum(len(phase.get("verdict_routes", [])) for phase in phases)
    return {
        "path": str(path),
        "sha256": sha256(path),
        **sizes,
        "version": value.get("version"),
        "phase_count": len(phases),
        "phase_bytes": phase_sizes,
        "required_field_counts": required_fields,
        "max_required_fields": max(required_fields.values(), default=0),
        "route_count": route_count,
        "semantic_coverage": semantic_coverage(value),
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--repo", type=Path, default=Path(__file__).resolve().parents[1])
    parser.add_argument(
        "--output-dir",
        type=Path,
        default=WORKSPACE_ROOT / "tect/programs/tect-substrate-daemon-v1",
    )
    args = parser.parse_args()
    legacy_path = args.repo / "crates/host/pipeline-definitions/lightweight-tdd-0.4.0-native.skills.1.json"
    current_path = args.repo / "crates/host/pipeline-definitions/lightweight-tdd-0.7.0-native.k1k5.json"
    legacy = definition_metrics(legacy_path)
    current = definition_metrics(current_path)

    missing = [name for name, present in current["semantic_coverage"].items() if not present]
    disappeared = [
        name
        for name, present in legacy["semantic_coverage"].items()
        if present and not current["semantic_coverage"].get(name, False)
    ]
    assertions = {
        "semantic_categories_present": not missing,
        "legacy_semantics_not_disappeared": not disappeared,
        "v07_definition_within_p9_local_target": current["compact_bytes"] <= P9_LOCAL_TARGETS["definition_bytes"],
        "v07_phases_within_p9_local_target": max(current["phase_bytes"].values(), default=0) <= P9_LOCAL_TARGETS["phase_bytes"],
        "v07_fields_within_plan_target": current["max_required_fields"] <= 8,
        "frame_bound_for_definition_and_phases": max(
            [current["compact_bytes"], *current["phase_bytes"].values()]
        )
        <= P9_LOCAL_TARGETS["frame_bytes"],
    }
    if missing or disappeared or not all(assertions.values()):
        raise AssertionError(
            json.dumps({"missing": missing, "disappeared": disappeared, "assertions": assertions}, indent=2)
        )

    runtime = {
        "provider_or_mcp_metrics": "unavailable: no live provider/MCP execution in this harness",
        "database_metrics": "unavailable: no live PostgreSQL execution in this harness",
        "benchmark_metrics": "unavailable: source-only deterministic smoke; no model timing/cost/token replay",
        "environment_variables_seen": sorted(k for k in os.environ if k.startswith(("TECT_TEST_", "DATABASE_", "PG"))),
    }
    result = {
        "harness": "agent-economy-local-parity-v1",
        "units": "UTF-8 bytes after compact JSON serialization",
        "legacy_v06": legacy,
        "current_v07": current,
        "semantic_categories": sorted(SEMANTIC_CATEGORIES),
        "missing_categories": missing,
        "disappeared_categories": disappeared,
        "p9_local_targets": P9_LOCAL_TARGETS,
        "assertions": assertions,
        "runtime_limits": runtime,
    }
    args.output_dir.mkdir(parents=True, exist_ok=True)
    json_path = args.output_dir / "wp7-local-parity-harness-20260919.json"
    md_path = args.output_dir / "wp7-local-parity-harness-20260919.md"
    json_path.write_text(json.dumps(result, indent=2, ensure_ascii=False) + "\n")
    lines = [
        "# WP7 local parity harness evidence",
        "",
        "Generated by `scripts/agent_economy_local_parity.py`.",
        "This is deterministic source-only evidence; it does not prove live DB, MCP, provider, or model benchmark behavior.",
        "",
        "## Inputs and sizes",
        "",
        "| Definition | File bytes | Compact bytes | Phases | Max required fields | Routes |",
        "|---|---:|---:|---:|---:|---:|",
        f"| v0.6 proxy | {legacy['file_bytes']} | {legacy['compact_bytes']} | {legacy['phase_count']} | {legacy['max_required_fields']} | {legacy['route_count']} |",
        f"| v0.7 K1-K5 | {current['file_bytes']} | {current['compact_bytes']} | {current['phase_count']} | {current['max_required_fields']} | {current['route_count']} |",
        "",
        "v0.7 phase compact bytes: " + ", ".join(f"{k}={v}" for k, v in current["phase_bytes"].items()) + ".",
        "",
        "## Assertions",
        "",
    ]
    lines.extend(f"- `{name}`: {'PASS' if value else 'FAIL'}" for name, value in assertions.items())
    lines += [
        "",
        "Semantic categories: " + ", ".join(sorted(SEMANTIC_CATEGORIES)) + ".",
        "Missing categories: none." if not missing else "Missing categories: " + ", ".join(missing) + ".",
        "Disappeared categories: none." if not disappeared else "Disappeared categories: " + ", ".join(disappeared) + ".",
        "",
        "## Runtime limits",
        "",
        *[f"- {key}: {value}" for key, value in runtime.items() if key != "environment_variables_seen"],
        "- No runtime/provider/DB claim is made by this artifact.",
        "",
        f"Machine-readable result: `{json_path}`.",
    ]
    md_path.write_text("\n".join(lines) + "\n")
    print(json.dumps({"json": str(json_path), "markdown": str(md_path), "assertions": assertions}, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
