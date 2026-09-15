"""Assertions for native pipeline execution in the existing five-tool acceptance."""

from __future__ import annotations

import json
import hashlib
import uuid
from typing import Any, Callable


LIGHTWEIGHT_PHASES = [
    "slice-lightweight-entry-gate",
    "slice-lightweight-intent-capture",
    "slice-lightweight-context-loader",
    "slice-workspace-preflight-lite",
    "slice-lightweight-contract-writer",
    "slice-lightweight-escalation-checker",
    "slice-test-target-selector",
    "slice-lightweight-pre-implementation-review",
    "slice-tdd-cycle-runner",
    "slice-implementation-note-writer",
    "slice-lightweight-verification-runner",
    "slice-deploy-impact-checker",
    "slice-lightweight-result-writer",
    "slice-lightweight-promotion-router",
    "slice-lightweight-maintenance-and-handoff",
]


def ok(call: Callable, tool: str, route: str, params: dict[str, Any]) -> dict[str, Any]:
    payload, failed = call(tool, {"route": route, "params": params})
    if failed:
        code = payload.get("error", {}).get("code")
        raise AssertionError(f"{route} failed: {code}")
    return payload


def begin_params(
    scope_id: str,
    slice_id: str,
    slice_revision: int,
    *,
    request_id: str | None = None,
    delivery_mode: str | None = None,
    qualification_reason: str = "Bounded Lightweight fixture with finite local proof.",
) -> dict[str, Any]:
    params = {
        "request_id": request_id or str(uuid.uuid4()),
        "scope_id": scope_id,
        "slice_id": slice_id,
        "slice_revision": slice_revision,
        "qualification_reason": qualification_reason,
    }
    if delivery_mode is not None:
        params["delivery_mode"] = delivery_mode
    return params


def outcome(payload: dict[str, Any], name: str) -> dict[str, Any]:
    value = payload.get(name)
    if not isinstance(value, dict):
        raise AssertionError(f"missing {name} pipeline outcome")
    return value


def assert_lightweight_whole_context(context: dict[str, Any], check: Callable) -> None:
    definition = context.get("definition", {})
    phases = definition.get("phases", [])
    delivered = context.get("delivered_phases", [])
    phase_ids = [phase.get("id") for phase in phases]
    delivered_ids = [phase.get("id") for phase in delivered]
    instructions = [
        instruction
        for phase in phases
        for field in ("instructions", "skills", "resources")
        for instruction in phase.get(field, [])
    ]
    check(
        "Lightweight whole delivery binds all fifteen ordered phases",
        definition.get("kind") == "slice.lightweight-tdd-development"
        and definition.get("default_mode") == "whole"
        and phase_ids == LIGHTWEIGHT_PHASES
        and delivered_ids == LIGHTWEIGHT_PHASES,
        {
            "definition_kind": definition.get("kind"),
            "phase_ids": phase_ids,
            "delivered_phase_ids": delivered_ids,
        },
    )
    check(
        "every delivered Lightweight instruction and skill has exact executable content",
        bool(instructions)
        and all(
            isinstance(item.get("id"), str)
            and item["id"].strip()
            and isinstance(item.get("version"), str)
            and item["version"].strip()
            and isinstance(item.get("digest"), str)
            and item["digest"].strip()
            and isinstance(item.get("body"), str)
            and item["body"].strip()
            and isinstance(item.get("origin_refs"), list)
            and item["origin_refs"]
            for item in instructions
        ),
        {
            "instruction_count": len(instructions),
            "identity_digest": _instruction_identity_digest(instructions),
        },
    )
    check(
        "actual pipeline delivery excludes Slice-candidate design rules",
        all(
            rule not in json.dumps(context)
            for rule in (
                "vertical-provable-slices",
                "no-unrequested-or-unauthorized-work",
                "autonomous-local-technical-decisions",
                "no-product-test-harness-work",
            )
        ),
        {"pipeline_run_id": context.get("run", {}).get("id")},
    )


def skill_reads(phase: dict[str, Any]) -> list[dict[str, str]]:
    return [
        {
            "instruction_id": skill["id"],
            "version": skill["version"],
            "digest": skill["digest"],
        }
        for skill in phase.get("skills", [])
    ]


def resource_reads(phase: dict[str, Any]) -> list[dict[str, str]]:
    return [
        {
            "instruction_id": resource["id"],
            "version": resource["version"],
            "digest": resource["digest"],
        }
        for resource in phase.get("resources", [])
    ]


def consumed_outputs(context: dict[str, Any]) -> list[dict[str, Any]]:
    current_ordinal = context["run"]["current_phase_ordinal"]
    current = {
        (item["phase_id"], item["revision"]): item
        for item in context.get("outputs", [])
        if item.get("stale") is False
    }
    consumed = []
    for binding in context.get("bindings", []):
        if binding.get("stale") is not False or binding["phase_ordinal"] >= current_ordinal:
            continue
        output = current[(binding["phase_id"], binding["output_revision"])]
        if output["digest"] != binding["output_digest"]:
            raise AssertionError("current output body does not match its pinned binding")
        consumed.append(
            {
                "phase_id": binding["phase_id"],
                "output_revision": binding["output_revision"],
                "digest": output["digest"],
            }
        )
    return consumed


def consumed_inputs(context: dict[str, Any]) -> list[dict[str, Any]]:
    phase_id = context["run"]["current_phase_id"]
    return [
        {"input_id": item["id"], "sequence": item["sequence"], "digest": item["digest"]}
        for item in context.get("inputs", [])
        if item.get("phase_id") == phase_id
    ]


def phase_output(
    phase: dict[str, Any],
    consumed: list[dict[str, Any]],
    outcome_name: str = "completed",
    transition: str = "continue",
) -> dict[str, Any]:
    marker = phase["id"]
    fields = {
        field: f"isolated acceptance evidence for {marker}"
        for field in phase.get("required_fields", [])
    }
    if marker == "slice-tdd-cycle-runner":
        fields.update(
            {
                "red_exit_code": "1",
                "green_exit_code": "0",
                "selected_test_identity_recorded": "true",
                "red_command_evidence_recorded": "true",
                "red_failure_observed": "true",
                "source_change_identity_recorded": "true",
                "green_command_evidence_recorded": "true",
                "green_pass_observed": "true",
                "same_target_binding_verified": "true",
            }
        )
    if marker == "slice-lightweight-verification-runner":
        fields.update(
            {
                "focused_exit_code": "0",
                "affected_exit_code": "0",
                "focused_proof_disposition_recorded": "true",
                "affected_proof_disposition_recorded": "true",
                "command_evidence_or_blocker_recorded": "true",
                "proof_target_binding_or_gap_recorded": "true",
                "verification_receipt_complete": "true",
            }
        )
    if marker == "slice-deploy-impact-checker":
        fields["deploy_impact_decision"] = "no_deploy_required"
    route = next(
        (
            item
            for item in phase.get("verdict_routes", [])
            if item.get("outcome") == outcome_name and item.get("transition") == transition
        ),
        None,
    )
    output: dict[str, Any] = {
        "body": (
            f"Isolated acceptance receipt for {marker}. The caller reports this "
            "structural evidence; the backend does not assert semantic truth."
        ),
        "producer_context_id": f"isolated-acceptance:{marker}",
        "fields": fields,
        "dispositions": list(
            route.get("dispositions", []) if route else phase.get("required_dispositions", [])
        ),
        "skill_reads": skill_reads(phase),
        "resource_reads": resource_reads(phase),
        "reference": f"isolated-acceptance/{marker}.md",
    }
    verdicts = phase.get("allowed_verdicts", [])
    if route:
        output["verdict"] = route["verdict"]
    elif verdicts:
        output["verdict"] = verdicts[0]
    review = next(
        (
            constraint
            for constraint in phase.get("output_constraints", [])
            if constraint.get("kind") == "engineering_review"
        ),
        None,
    )
    if review is not None:
        stage = review["stage"]
        file = {
            "path": "src/fixture.rs",
            "content_kind": "behavioral",
            "line_count": 20,
            "count_basis": "observed" if stage == "implementation" else "estimate",
            "responsibility": "Own the bounded fixture behavior.",
        }
        if stage == "implementation":
            file["content_digest"] = hashlib.sha256(b"fixture").hexdigest()
        report = {
            "stage": stage,
            "rules_digest": review["standards_resource_digest"],
            "verdict": "pass",
            "reviewed_outputs": consumed,
            "source_basis": "Current durable predecessor outputs for this fixture.",
            "assessments": [
                {
                    "rule_id": f"ENG-{number:02}",
                    "status": "satisfied",
                    "rationale": "The fixture supplies current concrete evidence.",
                    "evidence_refs": [f"fixture:{marker}"],
                }
                for number in range(1, 11)
            ],
            "findings": [],
            "files": [file],
            "summary": "The fixture conforms to the pinned engineering standards.",
        }
        body = json.dumps(report, separators=(",", ":"))
        output["artifacts"] = [
            {
                "name": "engineering-review.json",
                "media_type": "application/json",
                "digest": hashlib.sha256(body.encode()).hexdigest(),
                "body": body,
                "reference": f"isolated-acceptance/{marker}/engineering-review.json",
            }
        ]
    return output


def completion_params(
    context: dict[str, Any],
    *,
    outcome_name: str = "completed",
    transition: str = "continue",
    terminal_result: dict[str, Any] | None = None,
    publish_blocked_result: bool = False,
) -> dict[str, Any]:
    phase_id = context["run"]["current_phase_id"]
    phase = next(item for item in context["definition"]["phases"] if item["id"] == phase_id)
    consumed = consumed_outputs(context)
    output = phase_output(phase, consumed, outcome_name, transition)
    if phase.get("fresh_reviewer_input") is True:
        producer_context_ids = sorted(
            {
                item["producer_context_id"]
                for item in context.get("outputs", [])
                if item.get("stale") is False
            }
        )
        output["reviewer_context"] = {
            "reviewer_identity": "reported-independent-reviewer",
            "reviewer_context_id": output["producer_context_id"],
            "producer_context_ids": producer_context_ids,
            "fresh_input": True,
        }
    params = {
        "request_id": str(uuid.uuid4()),
        "run_id": context["run"]["id"],
        "run_revision": context["run"]["revision"],
        "phase_id": phase_id,
        "outcome": outcome_name,
        "transition": transition,
        "output": output,
        "consumed_outputs": consumed,
        "consumed_inputs": consumed_inputs(context),
        "publish_blocked_result": publish_blocked_result,
    }
    if terminal_result is not None:
        params["terminal_result"] = terminal_result
    return params


def _instruction_identity_digest(instructions: list[dict[str, Any]]) -> str:
    identities = [
        [item.get("id"), item.get("version"), item.get("digest")]
        for item in instructions
    ]
    encoded = json.dumps(identities, sort_keys=True, separators=(",", ":")).encode()
    return hashlib.sha256(encoded).hexdigest()
