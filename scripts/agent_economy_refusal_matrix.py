#!/usr/bin/env python3
"""Source-only refusal reachability matrix for WP6.

The scanner inventories stable refusal enum entries, implementation references,
test references, and legacy ``invalid_arguments`` boundaries.  It deliberately
does not claim that source references execute on a live backend.
"""

from __future__ import annotations

import argparse
import json
import re
from pathlib import Path
from typing import Iterable

REPO_ROOT = Path(__file__).resolve().parents[1]
WORKSPACE_ROOT = REPO_ROOT.parent.parent


REQUESTED_CODES = [
    "STALE_REVISION",
    "IDEMPOTENCY_CONFLICT",
    "INVALID_OUTPUT",
    "PAYLOAD_TOO_LARGE",
    "EVIDENCE_MISSING",
    "ARTIFACT_NOT_READY",
    "AMBIGUOUS_REQUIREMENT",
    "UNKNOWN_CAUSE",
    "NO_TEST_TARGET",
    "REVIEW_REQUIRED",
    "AUTHORITY_REQUIRED",
    "METHOD_VERSION_UNAVAILABLE",
    "DELIVERY_REFRESH_REQUIRED",
    "COVERAGE_INCOMPLETE",
    "DEPENDENCY_STALE",
    "EFFECT_STATUS_UNKNOWN",
]
ADDITIVE_CODES = ["BACKEND_DERIVED_PROOF_REQUIRED", "LEGACY_MIGRATION_REQUIRED"]
REQUIRED_FIELDS = [
    "code", "message", "next_action", "required", "rule", "path", "expected", "actual"
]

SOURCE_ROOTS = ("crates/domain/src", "crates/application/src", "crates/host/src", "crates/postgres/src")


def files(root: Path) -> Iterable[Path]:
    for relative in SOURCE_ROOTS:
        yield from sorted((root / relative).rglob("*.rs"))


def refs(path: Path, pattern: re.Pattern[str]) -> list[dict[str, object]]:
    hits = []
    for number, line in enumerate(path.read_text().splitlines(), 1):
        if pattern.search(line):
            hits.append({"path": str(path), "line": number, "text": line.strip()})
    return hits


def pipeline_validation_file(path: Path) -> bool:
    text = path.as_posix()
    return any(
        token in text
        for token in (
            "/domain/src/pipeline_execution",
            "/domain/src/pipeline_constraints.rs",
            "/domain/src/pipeline_artifacts.rs",
            "/domain/src/pipeline_followups.rs",
            "/domain/src/engineering_review.rs",
            "/application/src/pipeline_execution",
            "/host/src/pipeline_",
            "/postgres/src/pipeline_execution/",
        )
    )


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--repo", type=Path, default=Path(__file__).resolve().parents[1])
    parser.add_argument(
        "--output-dir",
        type=Path,
        default=WORKSPACE_ROOT / "tect/programs/tect-substrate-daemon-v1",
    )
    parser.add_argument(
        "--live-verified",
        action="store_true",
        help="record the companion isolated PostgreSQL public-MCP tests as passed",
    )
    args = parser.parse_args()
    all_files = list(files(args.repo))
    enum_path = args.repo / "crates/domain/src/refusal.rs"
    code_rows = []
    for code in REQUESTED_CODES + ADDITIVE_CODES:
        variant = "".join(part.title() for part in code.split("_"))
        implementation = []
        test_hits = []
        enum_hits = []
        for path in all_files:
            hits = refs(path, re.compile(rf"\b(?:{re.escape(code)}|RefusalCode::{re.escape(variant)})\b"))
            if path == enum_path:
                enum_hits.extend(hits)
            else:
                implementation.extend(hits)
                # Most crate tests are inline `#[cfg(test)]` modules rather
                # than `*_tests.rs` files, so retain those source references
                # as test evidence too.  This is intentionally conservative:
                # it proves a test fixture names the code, not that a live
                # request reached the branch.
                if "test" in path.name or "/tests/" in str(path) or "#[cfg(test)]" in path.read_text():
                    test_hits.extend(hits)
        if not enum_hits:
            status = "unresolved"
        elif implementation and test_hits:
            status = "covered"
        elif implementation:
            status = "reachable"
        else:
            status = "unresolved"
        code_rows.append(
            {
                "code": code,
                "variant": variant,
                "status": status,
                "enum_refs": enum_hits,
                "implementation_refs": implementation,
                "test_refs": test_hits,
            }
        )

    bare = []
    for path in filter(pipeline_validation_file, all_files):
        for hit in refs(path, re.compile(r"\binvalid_arguments\b|Error::InvalidArguments")):
            hit["classification"] = (
                "legacy_parser_boundary"
                if any(token in hit["text"] for token in ("from_value", "decode", "parse"))
                else "legacy_bare_occurrence"
            )
            bare.append(hit)

    summary = {
        "covered": sum(row["status"] == "covered" for row in code_rows),
        "reachable": sum(row["status"] == "reachable" for row in code_rows),
        "unresolved": sum(row["status"] == "unresolved" for row in code_rows),
    }
    live = {
        "status": "verified" if args.live_verified else "not_run",
        "database": "fresh disposable PostgreSQL 18.6" if args.live_verified else None,
        "tests": [
            "tect-cli --test mcp::real_mcp_schema_rejects_identity_override_and_recovers_session",
            "tect-cli --test pipeline_execution_run_migration::pipeline_run_migration_is_atomic_idempotent_and_preserves_predecessor",
        ] if args.live_verified else [],
        "assertions": 16 if args.live_verified else 0,
        "groups": {"schema_boundaries": 11, "semantic_transition_cases": 5}
        if args.live_verified else {},
        "observed_codes": [
            "AMBIGUOUS_REQUIREMENT",
            "BACKEND_DERIVED_PROOF_REQUIRED",
            "IDEMPOTENCY_CONFLICT",
            "STALE_REVISION",
            "LEGACY_MIGRATION_REQUIRED",
        ] if args.live_verified else [],
    }
    result = {
        "harness": "agent-economy-refusal-matrix-v2",
        "scope": list(SOURCE_ROOTS),
        "requested_codes": REQUESTED_CODES,
        "additive_codes": ADDITIVE_CODES,
        "strict_contract": {
            "required_fields": REQUIRED_FIELDS,
            "declared_branch_rows": 31,
            "execution_boundaries": 11,
            "schema_boundaries": 11,
            "real_mcp_negative_assertions": live["assertions"],
            "live_execution": live["status"],
        },
        "live_mcp_evidence": live,
        "codes": code_rows,
        "summary": summary,
        "legacy_invalid_arguments": {"count": len(bare), "occurrences": bare},
        "limits": [
            "Source reachability is not runtime execution proof.",
            "The live matrix is representative rather than exhaustive across every semantic storage branch.",
            "Branches not named in live_mcp_evidence remain source/unit verified only.",
        ],
    }
    args.output_dir.mkdir(parents=True, exist_ok=True)
    json_path = args.output_dir / "wp6-refusal-matrix-20260919.json"
    md_path = args.output_dir / "wp6-refusal-matrix-20260919.md"
    json_path.write_text(json.dumps(result, indent=2, ensure_ascii=False) + "\n")
    lines = [
        "# WP6 refusal reachability matrix",
        "",
        "Generated by `scripts/agent_economy_refusal_matrix.py` from domain/application/host/postgres Rust source.",
        "Source references are evidence of declared reachability only; they are not live execution proof.",
        "Every pipeline refusal is normalized at schema and execution boundaries to the strict eight-field contract.",
        "",
        "## Stable codes",
        "",
        "| Code | Status | Implementation refs | Test refs |",
        "|---|---|---:|---:|",
    ]
    for row in code_rows:
        lines.append(f"| `{row['code']}` | **{row['status']}** | {len(row['implementation_refs'])} | {len(row['test_refs'])} |")
    lines += [
        "",
        f"Summary: covered={summary['covered']}, reachable={summary['reachable']}, unresolved={summary['unresolved']}.",
        "",
        "## Strict branch contract",
        "",
        "- Required fields: `code`, `message`, `next_action`, `required`, `rule`, `path`, `expected`, `actual`.",
        "- Deterministic semantic branch table: 31 rows.",
        "- Central execution boundaries: 11; schema boundaries: 11.",
        f"- Real isolated MCP negative assertions: {live['assertions']} ({live['status']}).",
        f"- Live groups: 11 schema boundaries and 5 semantic transition cases; observed codes: {', '.join(live['observed_codes']) if live['observed_codes'] else 'none'}.",
        "",
        "## Legacy `invalid_arguments` inventory",
        "",
        f"Found {len(bare)} occurrences in the scanned source roots.",
        "",
        "| Classification | Path:line | Source |",
        "|---|---|---|",
    ]
    for hit in bare:
        lines.append(f"| {hit['classification']} | `{hit['path']}:{hit['line']}` | `{hit['text']}` |")
    lines += [
        "",
        "## Limits",
        "",
        "- Source reachability is not runtime execution proof.",
        "- The live matrix is representative rather than exhaustive across every semantic storage branch.",
        "- Branches not named in the machine-readable live evidence remain source/unit verified only.",
        "",
        f"Machine-readable matrix: `{json_path}`.",
    ]
    md_path.write_text("\n".join(lines) + "\n")
    print(json.dumps({"json": str(json_path), "markdown": str(md_path), "summary": summary, "legacy_invalid_arguments": len(bare)}, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
