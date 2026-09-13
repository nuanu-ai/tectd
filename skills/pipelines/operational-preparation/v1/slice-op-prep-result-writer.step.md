---
id: "slice-op-prep-result-writer"
kind: "internal_step"
owner_family: "slice_variant"
pipeline_id: "slice.operational-preparation"
step_id: "slice-op-prep-result-writer"
entry_gate: false
public_trigger: false
native_skill_discovery: false
body_path: "capabilities/instructions/pipelines/slice-variants/operational-preparation/slice-op-prep-result-writer.step.md"
source_manifest: "capabilities/pipelines/slice-variants/operational-preparation.pipeline.json"
legacy_skill_ref: "slice-op-prep-result-writer"
architecture_refs:
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#operational-preparation-and-operational-execution-variants"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19"
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#operational-and-hybrid-slice-variants"
---

# Slice Operational Preparation Result Writer

## Overview
This skill closes an Operational Preparation Slice by writing `result.md` from an already prepared operation package. The core rule is: preparation can be ready, blocked, or handed off, but it is never proof that the operation ran or that the target state changed.

## When to Use
Use this when Runtime selected `slice.operational-preparation`, the active step is `slice-op-prep-result-writer`, and the preparation package has reached closure after `operation-plan.md`, `rollback-plan.md`, `proof-contract.md`, and `handoff.md` are drafted or a missing authority/evidence gate blocks them.

Use it when `result.md` must say `prepared_not_executed`, `blocked_missing_authority`, or `handoff_ready`, including cases where read-only or dry-run evidence exists but no mutating action was authorized.

Do not use it for operational execution result truth. `slice-op-exec-result-writer` owns outcomes after authorized side effects, action logs, post-action validation, rollback, recovery, observation, or live/current proof. Route there only after explicit execution authority and variant transition.

## Source Contract
Grounding sources are `capabilities/pipelines/slice-variants/operational-preparation.pipeline.json` step `slice-op-prep-result-writer`, `capabilities/pipelines/slice-variants/operational-preparation.pipeline.json#step_graph.steps.slice-op-prep-result-writer.invokes.slice-op-prep-result-writer`, `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#operational-preparation-and-operational-execution-variants`, `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#operational-and-hybrid-slice-variants`, `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6`, `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19`, and `docs/architecture/master-plugin-target-architecture-capability-atom-shard-map.html#pipeline.slice.operational-preparation`.

The manifest step produces `result.md`, gates on `prepared_not_executed_truth_recorded`, fails by `block_operation_completed_claim`, and allows only `prepared_not_executed`, `blocked_missing_authority`, or `handoff_ready`. The preparation manifest forbids execution artifacts such as `execution-log.md`, `action-ledger.md`, `post-action-validation.md`, and `authority-confirmation.md`.

## Operating Procedure
1. Load the preparation closure packet: Slice identity, operation intent, authority boundary, current-state baseline, risk-impact notes, preflight checks, operation plan, rollback plan, proof contract, dry-run/read-only evidence if present, handoff package, deferred blockers, and any missing-source notes.
2. Prove the package exists before closure. Check that the operation target, desired final state, non-goals, preflight order, exact commands or user actions, expected outputs, stop conditions, rollback/recovery path, proof criteria, and next actor are present or explicitly blocked.
3. Classify truth without upgrading it. Mark `prepared_not_executed` when the package is complete enough for a user, team, or later approved agent to act. Mark `handoff_ready` when execution belongs to another actor and the package names where to paste results. Mark `blocked_missing_authority` when execute/deploy/write/seed/migrate permission is absent, ambiguous, or explicitly withheld.
4. Record proof posture. Treat read-only checks and dry runs as preparation evidence only; state that proof is not live execution proof unless a later operational execution Slice validates it. Name missing authority, evidence, target access, credentials, current-state freshness, preflight output, rollback detail, or proof criteria that prevent stronger closure.
5. Draft `result.md` with terminal state, prepared package inventory, authority boundary, not-executed statement, proof-not-live statement, handoff readiness, next actor, deferred blockers, missing evidence, residual risk, escalation route, and forbidden claims that must not be made from this preparation.
6. Route follow-up without performing it. Escalate to `slice.operational-execution` or `slice.hybrid-implementation-operation` only when authority and scope change; route reusable procedure candidates to promotion separately; route stale or incomplete preparation artifacts to maintenance or handoff.

## Outputs
The required output is `result.md`. It must include terminal state, prepared package inventory, explicit `prepared_not_executed` truth, authority status, proof posture, proof-not-live boundary, handoff readiness, next actor, missing authority/evidence, rollback gaps, deferred blockers, residual risk, escalation route, and forbidden claims.

Write the result as a preparation closure record with these fields:

- `prepared_artifacts`: list present and missing prep artifacts, including operation, rollback, proof, handoff, dry-run/read-only evidence, and missing prep.
- `safe_later_execution`: name what is safe to execute later only if the recorded authority, preflight, stop conditions, rollback path, and proof contract are still fresh.
- `unsafe_later_execution`: name what is unsafe until missing authority, stale state, credential access, rollback ambiguity, or proof gaps are resolved.
- `next_owner`: name the user, team, later authorized agent, maintenance owner, or promotion owner responsible for the next action.
- `forbidden_claims`: list claims this preparation cannot support, including preflight passed as final proof, operation ran, deploy happened, rollback verified, live proof exists, or target state changed.

Allowed terminal results are `prepared_not_executed`, `blocked_missing_authority`, and `handoff_ready`. This skill may mention promotion candidates, maintenance needs, or execution escalation, but it does not run commands, validate live target state, mutate workspaces, deploy, seed, migrate, rollback, write durable-domain truth, or promote a runbook.

## Verification
Trigger verification must select this skill only when an operational preparation Slice is at closure and needs `result.md` to record the prepared package truth. It must reject scenarios where the next work is still intent capture, authority declaration, current-state baselining, preflight design, command planning, rollback planning, proof-contract design, handoff assembly, operational execution, live validation, promotion, or maintenance repair.

Content verification checks that `result.md` distinguishes prepared from executed, states proof is not live/current execution proof, records handoff readiness, names deferred blockers, preserves missing authority or evidence, calls out residual risk and rollback gaps, blocks operation-completed claims, and explicitly routes operational execution result truth to `slice-op-exec-result-writer`.

Run `node tools/validate-internal-skill-body-quality.mjs --skill slice-op-prep-result-writer` and `node tools/validate-internal-skill-trigger-fixtures.mjs --skill slice-op-prep-result-writer`.

## Failure Modes
Stop or hand off when the preparation package is absent, required artifacts are missing, authority is ambiguous, the operation target or desired final state is unclear, current-state evidence is stale, preflight or rollback detail is incomplete, proof criteria are too vague, credentials would need to be exposed, or the requested result would imply the operation completed.

Use `blocked_missing_authority` when execute/deploy/write/seed/migrate permission is not present. Use `handoff_ready` when the package is complete but action or proof belongs to the user, team, or a later authorized execution Slice. Use `prepared_not_executed` only when the package is ready and the result keeps the not-executed boundary intact.

Do not say preflight passed, the operation ran, deploy happened, rollback succeeded, live proof exists, or current target state changed unless those facts come from a later operational execution or hybrid result writer. Preparation truth can only say what is ready, what is blocked, who owns the next step, and what evidence must be returned after action.
