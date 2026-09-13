---
id: "slice-procedure-result-writer"
kind: "internal_step"
owner_family: "slice_variant"
pipeline_id: "slice.custom-procedure-capture"
step_id: "slice-procedure-result-writer"
entry_gate: false
public_trigger: false
native_skill_discovery: false
body_path: "capabilities/instructions/pipelines/slice-variants/procedure-capture/slice-procedure-result-writer.step.md"
source_manifest: "capabilities/pipelines/slice-variants/procedure-capture.pipeline.json"
legacy_skill_ref: "slice-procedure-result-writer"
architecture_refs:
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#hybrid-implementation-operation-and-procedure-capture-variants"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19"
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#procedure-and-research-slice-variants"
---

# Procedure Capture Result Writer

## Overview

Use this skill at the result boundary for `slice.custom-procedure-capture`. It records terminal truth for a captured procedure: promoted, updated, rejected, blocked, or left for review. It is a recorder, not a promoter. It must not perform durable-domain mutation, accepted promotion overclaim, proposal writing, secret scrubbing, command execution, active skill creation, runbook publication, index update, or workspace cleanup.

The result is limited to the Procedure capture Slice. When Runtime has authority to write slice artifacts, the only owned write is `result.md`; otherwise return a `result.md`-ready record for the caller to place. Durable runbooks, knowledge pages, plugin skills, registries, and maintenance projections remain outside this skill.

## When to Use

Use this when a selected Procedure capture Slice is ready to close and the source inputs already exist: `source-event.md`, `captured-steps.md`, `normalized-procedure.md`, `existing-match-check.md`, `authority-risk.md`, `proof-contract.md`, `secret-safety.md`, `reuse-fit.md`, `procedure-proposal.md`, and `promotion-gate.md` when promotion was considered.

Select this skill for result truth only: a procedure was promoted by an authorized external step, an existing procedure or runbook was updated by an authorized external step, the candidate was rejected, the candidate is blocked, or the proposal must be left for review with a next owner.

Do not use this to execute an existing runbook, write the procedure proposal, scrub secrets, approve promotion, edit a canonical runbook, mutate durable KB/domain content, create or publish a skill, run maintenance, or decide the reusable procedure shape before upstream evidence exists. Route those cases to the earlier Procedure capture step, runbook-library promotion workflow, durable knowledge domain, skill-authoring pipeline, operational execution, or maintenance workflow.

## Source Contract

Ground the result in `capabilities/pipelines/slice-variants/procedure-capture.pipeline.json#step_graph.steps.slice-procedure-result-writer`. That manifest step produces `result.md`, gates on `result_truth_recorded`, and fails to `stop_or_handoff`. The Procedure capture completion contract requires proof before completion, highest validated proposal truth, no durable write, and no skill activation.

Architecture anchors are `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#hybrid-implementation-operation-and-procedure-capture-variants`, `docs/architecture/master-plugin-target-architecture-part-6c-stateful-domain-pipelines.html#cross-domain-routing`, `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6`, `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19`, and `docs/architecture/master-plugin-target-architecture-capability-atom-shard-map.html#pipeline.slice.procedure_capture.result.writer`.

Map result language to manifest truth without expanding authority:

- `promoted`: allowed only when separate authorized promotion proof exists. Preserve the manifest terminal `procedure_promoted_after_approval` and state this skill did not promote.
- `updated`: allowed only when separate authorized update proof exists, or record `left_for_review` when an existing runbook/procedure should be updated but has not been changed yet.
- `rejected`: map to `procedure_rejected` when reuse fit, safety, authority, duplicate risk, or proof makes the candidate unsuitable.
- `blocked`: map to `blocked_missing_source_event`, `blocked_secret_risk`, `blocked_duplicate_or_existing_match`, or another explicit blocker when required proof or authority is absent.
- `left_for_review`: map to `handoff_ready` or `deferred_pending_owner` when a candidate exists but a human/domain owner must decide promotion, update, rejection, or later review.

## Operating Procedure

1. Confirm the active Slice selected `slice.custom-procedure-capture` and this exact step. If the request is proposal writing, secret safety, execution, durable promotion, skill creation, or maintenance, stop and route it away.
2. Load only result inputs: source event, captured steps, normalized procedure, existing-match check, authority-risk record, proof contract, secret-safety result, reuse-fit result, procedure proposal, validation result when present, and promotion gate.
3. Classify the highest validated terminal truth without improving upstream artifacts: promoted, updated, rejected, blocked, or left for review. Do not convert "proposal exists" into "accepted" or "published."
4. Record proof level and gaps. Missing source event, missing proof contract, unresolved secret risk, unclear authority, unresolved duplicate risk, or absent next owner blocks completion or forces `left_for_review`.
5. State the mutation boundary in the result: whether any external authorized promotion or update already happened, the proof for it, and that this skill performed no durable write, no runbook publication, no KB mutation, no active skill creation, and no execution.
6. Assign next owner and next route for every non-final state: user review, runbook library promotion request, durable knowledge domain review, existing runbook update owner, skill-authoring pipeline, follow-up Slice, or maintenance/handoff step.
7. Write or return the `result.md` record only after `result_truth_recorded` can pass. Stop at the result boundary; do not chain into promotion, update, cleanup, or index refresh.

## Outputs

Produce a concise `result.md` record with this shape:

- `terminal_truth`: one of `promoted`, `updated`, `rejected`, `blocked`, or `left_for_review`, plus the manifest term when useful.
- `source_inputs`: exact upstream artifacts used, including proposal and promotion-gate inputs.
- `proof_level`: proof present, proof gaps, and whether missing proof blocks completion.
- `durable_write_status`: no durable write by this skill; cite external authorized proof if promotion or update already happened.
- `publication_status`: no active skill/runbook publication by this skill.
- `promotion_readiness`: ready, not ready, already externally promoted, update required, rejected, blocked, or review required.
- `next_owner` and `next_route`: required for blocked, updated-required, or left-for-review results.
- `residual_risks`: duplicate risk, one-off context, safety caveats, stale inputs, unresolved authority, or deferred work.

The result must distinguish candidate existence from durable artifact existence. It may reference `procedure-proposal.md`, `runbook-draft.md`, `command-recipe.md`, `proof-order-template.md`, or `skill-candidate.md` as proposal artifacts, but it must not claim a live runbook, active skill, or canonical KB page unless separate authorized proof is cited.

## Verification

Before calling the result complete, verify:

1. `result.md` states the highest validated proposal truth and uses one of the five terminal truth states.
2. `result_truth_recorded` is satisfied: source inputs are named, proof level is explicit, mutation status is explicit, and next owner/route exists when needed.
3. No line claims this skill performed promotion, update, proposal writing, secret scrubbing, execution, durable-domain mutation, runbook publication, active skill creation, or maintenance.
4. Promotion and update claims cite external authorized proof; otherwise the result is `left_for_review` or `blocked`.
5. Blocked states are explicit when source event, captured steps, proof contract, secret safety, authority, duplicate/existing-match, reuse fit, validation, or promotion-gate evidence is missing.

Run `node tools/validate-internal-skill-body-quality.mjs --skill slice-procedure-result-writer` and `node tools/validate-internal-skill-trigger-fixtures.mjs --skill slice-procedure-result-writer` after edits to this skill or its fixtures.

## Failure Modes

Block when the source event is absent, captured steps are untrusted, proof contract is missing, secret risk is unresolved, authority is unclear, duplicate risk points to an existing artifact with no update owner, promotion/update proof is asserted but not evidenced, or the user asks this skill to mutate durable truth.

Leave for review when the candidate appears useful but the owner, durable target, approval, review cadence, or update-vs-new decision is unresolved. Route actual durable writes to runbook-library, durable knowledge, protocol/security/operations/product domains, or skill-authoring workflows as appropriate.

Reject when reuse fit, safety, authority, duplicate analysis, or proof quality shows the captured workflow should not become durable guidance. Never use rejection to hide missing evidence; use blocked when required proof is missing.

Stop immediately if the work becomes execution, secret scrubbing, proposal writing, promotion approval, canonical artifact editing, index/front-door update, skill creation, deployment, cleanup, or live-system command work. This skill records terminal truth only.
