"""Assertions for native pipeline execution in the existing five-tool acceptance."""

from __future__ import annotations

import json
import hashlib
import uuid
from typing import Any, Callable
from common import hydrate_pipeline_payload


LIGHTWEIGHT_KIND = "slice.lightweight-tdd-development"
LIGHTWEIGHT_VERSION = "0.7.1-native.k1k5"
LIGHTWEIGHT_PINS = {
    "0.7.1-native.k1k5": "93df97f4cb4458a18411b76005b29025a56234dc47650e4147ac5fdab3d30d89",
    "0.7.0-native.k1k5": "7f5dd6a4503078538d45d0c90c83fdcd896ff1216167556ff9bd0424f826aab0",
}
LIGHTWEIGHT_PHASES = ["K1", "K2", "K3", "K4", "K5"]
FIXTURE_BOUNDARY = "Structural fixture only: no commands executed, independent semantic QA, deployment, live verification or paid acceptance."



def ok(call: Callable, tool: str, route: str, params: dict[str, Any]) -> dict[str, Any]:
    payload, failed = call(tool, {"route": route, "params": params})
    if failed:
        code = payload.get("error", {}).get("code")
        raise AssertionError(f"{route} failed: {code}")
    return hydrate_pipeline_payload(call, payload)


def begin_params(
    scope_id: str,
    slice_id: str,
    slice_revision: int,
    *,
    request_id: str | None = None,
    delivery_mode: str | None = None,
    definition_version: str | None = None,
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
    if definition_version is not None:
        params["definition_version"] = definition_version
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
        "Lightweight whole retrieval binds exactly K1-K5 and distinguishes snapshot references",
        definition.get("kind") == "slice.lightweight-tdd-development"
        and definition.get("default_mode") == "phasewise"
        and definition.get("allowed_modes") == ["whole", "phasewise"]
        and definition.get("version") == LIGHTWEIGHT_VERSION
        and definition.get("digest") == LIGHTWEIGHT_PINS[LIGHTWEIGHT_VERSION]
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
    # Compact phasewise v0.7 contexts intentionally omit output bodies.  The
    # returned attempt/checkpoint ledger still pins each output by output_id,
    # revision, and digest, so use that durable identity as the fallback for
    # the consumed binding instead of requiring the legacy body tuple.
    checkpoints_by_id = {
        item["output_id"]: item
        for item in context.get("attempts", [])
        if item.get("stale_dependency") is not True and item.get("output_id")
    }
    checkpoints_by_key = {
        (item["phase_id"], item["output_revision"]): item
        for item in context.get("attempts", [])
        if item.get("stale_dependency") is not True
        and item.get("output_revision") is not None
    }
    consumed = []
    for binding in context.get("bindings", []):
        if binding.get("stale") is not False or binding["phase_ordinal"] >= current_ordinal:
            continue
        output = current.get((binding["phase_id"], binding["output_revision"]))
        if output is not None:
            if output["digest"] != binding["output_digest"]:
                raise AssertionError("current output body does not match its pinned binding")
            digest = output["digest"]
        else:
            checkpoint = checkpoints_by_id.get(binding.get("output_id"))
            if checkpoint is None:
                checkpoint = checkpoints_by_key.get(
                    (binding["phase_id"], binding["output_revision"])
                )
            if checkpoint is None:
                raise AssertionError(
                    "compact context is missing the pinned output checkpoint"
                )
            if checkpoint.get("output_digest") != binding["output_digest"]:
                raise AssertionError("returned checkpoint does not match its pinned binding")
            digest = checkpoint["output_digest"]
        consumed.append(
            {
                "phase_id": binding["phase_id"],
                "output_revision": binding["output_revision"],
                "digest": digest,
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


def fixture_command_receipt(status: str, scope: str, target: str) -> str:
    return json.dumps({"command": f"STRUCTURAL_FIXTURE_NOT_EXECUTED:{scope}",
        "target": target, "status": status, "exit_code": 1 if status == "failed_as_expected" else 0,
        "fresh": True, "skipped": False, "scopes": [scope]}, separators=(",", ":"))


def lightweight_phase_output(phase: dict[str, Any], outcome_name: str, transition: str) -> dict[str, Any]:
    """Build labeled structural fixtures from the actual five checkpoint contracts."""
    marker = phase["id"]
    if marker not in LIGHTWEIGHT_PHASES:
        raise AssertionError("current Lightweight fixture cannot select a historical phase")
    route = next((r for r in phase["verdict_routes"] if r["outcome"] == outcome_name and r["transition"] == transition), None)
    if route is None:
        raise AssertionError(f"{marker} has no declared {outcome_name}/{transition} route")
    fields = {field: f"STRUCTURAL_FIXTURE_NOT_EXECUTED:{marker}:{field}" for field in phase["required_fields"]}
    values = {
        "K1": {"fit":"bounded_understood", "parent":"current_confirmed", "preflight":"current_clear", "authority":"authorized", "route":"none",
               "request":"Structural fixture for a bounded correction.", "acceptance_checks":"Fixture consumer-path regression plus affected checks."},
        "K2": {"source_provenance":"fixture:owned-source", "worktree_provenance":"fixture:isolated-worktree", "isolation":"confirmed", "ownership":"confirmed",
               "overlap":"clear", "target_proof_plan":"Fixture consumer-path focused and affected test plan.", "test_target":"fixture:consumer-path", "route":"none"},
        "K3": {"review_mode":"self", "findings":"Fixture self review considers whether the requested consumer path remains disconnected.",
               "verdict":route["verdict"], "reviewer":"structural-fixture-self", "rules_digest":phase["instructions"][0]["digest"], "missing_proof":"none", "next_owner":"none"},
        "K4": {"target_binding":"fixture:consumer-path", "red_receipt":fixture_command_receipt("failed_as_expected", "focused", "fixture:consumer-path"),
               "green_receipt":fixture_command_receipt("passed", "focused", "fixture:consumer-path"), "anti_pattern_review":"reviewed_clear", "authority_boundary":"authorized", "missing_proof":"none",
               "changes":"Fixture minimal correction; no source command executed.", "deviations":"none"},
        "K5": {"focused_proof":fixture_command_receipt("passed", "focused", "fixture:consumer-path"),
               "affected_proof":fixture_command_receipt("passed", "affected", "fixture:affected-path"),
               "deploy_impact":"no_deploy_required", "truth_level":"local_verified", "missing_proof":"none", "promotion":"no_promotion", "handoff":"none",
               "result":"Fixture local result carrier; no actual command or independent semantic verification."},
    }
    fields.update(values[marker])
    if route["verdict"] != "pass":
        if marker == "K1": fields["route"] = route["verdict"]
        if marker == "K2": fields["route"] = "escalate" if route["verdict"] == "escalate" else "defer"
        if "missing_proof" in fields: fields["missing_proof"] = f"fixture:{route['verdict']}"
        if marker == "K5": fields["truth_level"] = "fixture_not_verified"
    return {"body":FIXTURE_BOUNDARY, "producer_context_id":f"structural-fixture:{marker}", "fields":fields,
            "verdict":route["verdict"], "dispositions":list(route["dispositions"]), "reference":f"fixture:{marker}"}


def phase_output(
    phase: dict[str, Any],
    consumed: list[dict[str, Any]],
    outcome_name: str = "completed",
    transition: str = "continue",
    *, compact: bool = False,
) -> dict[str, Any]:
    if compact:
        return lightweight_phase_output(phase, outcome_name, transition)
    marker = phase["id"]
    fields = {
        field: f"isolated acceptance evidence for {marker}"
        for field in phase.get("required_fields", [])
    }
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
    revisit_phase_id: str | None = None,
    reviewer_context: dict[str, Any] | None = None,
    review_mode: str | None = None,
) -> dict[str, Any]:
    phase_id = context["run"]["current_phase_id"]
    phase = next(item for item in context["definition"]["phases"] if item["id"] == phase_id)
    version = context["run"].get("definition_version")
    lightweight = context["run"].get("definition_kind", context["definition"].get("kind")) == LIGHTWEIGHT_KIND
    if lightweight and version not in LIGHTWEIGHT_PINS:
        raise AssertionError("retired or unsupported Lightweight cannot be executed by this driver")
    compact_v07 = lightweight and version in LIGHTWEIGHT_PINS
    if compact_v07 and (context["definition"].get("version") != version
            or context["definition"].get("digest") != LIGHTWEIGHT_PINS[version]
            or context["run"].get("definition_digest") != LIGHTWEIGHT_PINS[version]
            or [p["id"] for p in context["definition"]["phases"]] != LIGHTWEIGHT_PHASES):
        raise AssertionError("Lightweight snapshot does not match its five-phase version pin")
    consumed = [] if compact_v07 else (context["consumed_outputs"] if "consumed_outputs" in context else consumed_outputs(context))
    output = phase_output(phase, consumed, outcome_name, transition, compact=compact_v07)
    if review_mode is not None:
        mode_constraint = next((c for c in phase.get("output_constraints", []) if c.get("kind") == "reviewer_context_mode"), None)
        if mode_constraint is None or review_mode not in {mode_constraint["self_value"], mode_constraint["independent_value"]}:
            raise AssertionError("review mode is not supported by this phase contract")
        output["fields"][mode_constraint["field"]] = review_mode
    requires_reviewer = phase.get("fresh_reviewer_input") is True or output["fields"].get("review_mode") == "independent"
    if requires_reviewer and reviewer_context is None:
        raise AssertionError("explicit reviewer_context required; authenticated independence, freshness and quality remain backend checks")
    if reviewer_context is not None:
        supported = {"reviewer_identity", "reviewer_context_id", "producer_context_ids", "fresh_input"}
        if not isinstance(reviewer_context, dict) or set(reviewer_context) - supported:
            raise AssertionError("reviewer_context contains unsupported contract fields")
        output["reviewer_context"] = json.loads(json.dumps(reviewer_context))
        if requires_reviewer and compact_v07:
            if not isinstance(reviewer_context.get("reviewer_identity"), str) or not reviewer_context["reviewer_identity"].strip():
                raise AssertionError("explicit reviewer_identity required for independent request construction")
            output["fields"]["reviewer"] = reviewer_context["reviewer_identity"]
            output["fields"]["findings"] = "Structural fixture review carrier; authenticated independence and semantic quality require backend checks."
    params = {
        "request_id": str(uuid.uuid4()),
        "run_id": context["run"]["id"],
        "run_revision": context["run"]["revision"],
        "phase_id": phase_id,
        "outcome": outcome_name,
        "transition": transition,
        "output": output,
        "publish_blocked_result": publish_blocked_result,
    }
    if not compact_v07:
        params["consumed_outputs"] = consumed
        params["consumed_inputs"] = context["consumed_inputs"] if "consumed_inputs" in context else consumed_inputs(context)
        if context.get("consumed_knowledge") is not None:
            params["consumed_knowledge"] = context["consumed_knowledge"]
    if revisit_phase_id is not None:
        route = next((r for r in phase.get("verdict_routes", []) if r["outcome"] == outcome_name and r["transition"] == transition), {})
        if revisit_phase_id not in phase.get("allowed_backward_to", []) or revisit_phase_id not in route.get("revisit_to", []):
            raise AssertionError("fixture requests an undeclared backward route")
        params["revisit_phase_id"] = revisit_phase_id
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


def rework_and_resume(call: Callable, context: dict[str, Any], target: str) -> dict[str, Any]:
    """Exercise an explicitly declared backward fixture route and exact-revision input."""
    returned = ok(call, "command", "slice.pipeline.phase.complete",
                  completion_params(context, outcome_name="waiting_input", revisit_phase_id=target))
    context = returned["context"]
    if context["run"]["current_phase_id"] != target or context["run"]["status"] != "waiting_input":
        raise AssertionError("backward fixture route did not reach its declared target")
    return ok(call, "command", "slice.pipeline.input", {
        "request_id":str(uuid.uuid4()), "run_id":context["run"]["id"],
        "run_revision":context["run"]["revision"], "phase_id":target,
        "input":"Structural fixture supplies rework resume input; no actual command executed."})["context"]
