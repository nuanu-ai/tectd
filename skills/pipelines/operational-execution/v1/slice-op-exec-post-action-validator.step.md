---
id: "slice-op-exec-post-action-validator"
kind: "internal_step"
owner_family: "slice_variant"
pipeline_id: "slice.operational-execution"
step_id: "slice-op-exec-post-action-validator"
entry_gate: false
public_trigger: false
native_skill_discovery: false
body_path: "capabilities/instructions/pipelines/slice-variants/operational-execution/slice-op-exec-post-action-validator.step.md"
source_manifest: "capabilities/pipelines/slice-variants/operational-execution.pipeline.json"
legacy_skill_ref: "slice-op-exec-post-action-validator"
architecture_refs:
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#operational-preparation-and-operational-execution-variants"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19"
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#operational-and-hybrid-slice-variants"
---

# slice-op-exec-post-action-validator

## Overview

Validate the real current state after an authorized `slice.operational-execution` action has already been attempted. This skill is a post-action proof gate, not an executor: no action execution, no retries, no deployment, no target mutation, no rollback, no promotion, no durable-domain write, no result truth writing, and no completion claim without current evidence.

The owning manifest is `capabilities/pipelines/slice-variants/operational-execution.pipeline.json` under `pipeline.slice.operational-execution`. The step is `slice-op-exec-post-action-validator`; it invokes this skill, produces `post-action-validation.md`, gates on `post_action_validation_recorded`, reaches `ready_for_next_step` only with target-matched proof, and otherwise stops or hands off through `stop_or_handoff`.

## When to Use

Use this after `slice-op-exec-action-runner` and checkpoint verification have recorded the attempted action in `execution-log.md` for the same bounded target named by the Slice.

Positive trigger boundary:
- an approved action is already recorded and the next gate is current proof;
- the proof contract requires logs, API, UI, database, chain, service health, version/config, queue/job, monitoring, or user-confirmation evidence;
- the result writer is blocked until `post-action-validation.md` separates verified state, missing proof, stale proof, wrong-target proof, and residual risk.

Do not select it for preparation-only work, action planning, preflight, running the next ledger command, retrying a failed operation, rollback/recovery execution, longer observation-window ownership, result writing after proof is already recorded, promotion routing, cleanup, or any request whose next safe owner is a human/operator rather than post-action validation.

## Source Contract

Classification: `skill_body`.

Ground this behavior in:
- `capabilities/registry/internal-skill-resolutions.json`
- `capabilities/registry/internal-skill-body-implementation-status.json`
- `capabilities/registry/internal-skill-fidelity-status.json`
- `capabilities/pipelines/slice-variants/operational-execution.pipeline.json`
- `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#operational-preparation-and-operational-execution-variants`
- `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#operational-and-hybrid-slice-variants`
- `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6`
- `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19`
- `docs/architecture/master-plugin-target-architecture-capability-atom-shard-map.html#pipeline.slice.operational_execution.post.action.validator`

The atom row is `pipeline.slice.operational_execution.post.action.validator`: a spine manifest step owned by `capabilities/pipelines/slice-variants/operational-execution.pipeline.json`, surfaced through `tect-work`, with proof relationship `required` and authority boundary `approval_required`. Its declared source summary is final proof from logs, endpoint, DB query, version, API smoke, chain event, UI check, or confirmation.

Required source_inputs are the selected Slice contract, `operation-intent.md`, `authority-confirmation.md`, `current-state.md`, `preflight.md`, `risk-stop-conditions.md`, `action-ledger.md`, `execution-log.md`, checkpoint output, proof contract, optional `rollback.md`, optional `observation-window.md`, and any retained evidence paths. Evidence must name the same target identity as the action: workspace, repo, branch, host, service, environment, database, account, chain, contract, endpoint, version, or UI surface.

## Operating Procedure

1. Load source_inputs and reconstruct the authorized target, desired final state, action timestamp, actor, command/tool call, checkpoint status, and proof contract.
2. Confirm that the action was actually attempted. If `execution-log.md` lacks the recorded attempt, stop and route to `slice-op-exec-action-runner` or handoff; do not validate a future command.
3. Rebuild exact target identity from the Slice, authority record, action ledger, execution log, and current-state baseline. If proof points at a different host, account, environment, branch, database, chain, contract, endpoint, UI, version, or service, record wrong-target proof and stop.
4. Enumerate required proof classes before collecting anything: logs, endpoint, live API, UI, database, chain receipt/event/balance, service health, version/config, queue/job state, monitoring signal, user confirmation, and any waived classes.
5. Gate evidence on authority. Use only read-only validation reads already allowed by the proof contract. Forbidden actions include action execution, retries, deploys, writes, migrations, restarts, cleanup, rollback/recovery, branch/worktree mutation, credential changes, traffic changes, promotion, or result truth writing.
6. Gate evidence on freshness. Proof must be newer than the action timestamp, tied to the relevant block/receipt id, or explicitly scoped to the post-action observation window. Stale logs, cached responses, pre-action rows, historical screenshots, or old chain state do not satisfy proof.
7. Capture each evidence item with source, timestamp or block/receipt id, target identity, observed value, expected value, source class, redaction status, and retained evidence path when available. Do not paste secrets, tokens, private keys, full credentials, or unrelated log dumps.
8. Compare observed state to expected state per proof class. Classify each row as `matched`, `partial`, `missing`, `stale`, `wrong_target`, `contradicted`, `unsafe_to_collect`, or `waived_by_contract`.
9. Evaluate proof gates. `post_action_validation_recorded` is true only when every required proof class is matched or explicitly waived, freshness is acceptable, target identity matches, and no stop condition remains open.
10. For async or delayed systems, record the current proof and use `observation_window_required` when a longer watch owns the next decision. Route to `slice-op-exec-observation-window-manager`; do not simulate the wait with unapproved retries.
11. If proof fails, contradicts expected state, or reveals a stop condition, route to `slice-op-exec-rollback-or-recovery-runner` only when rollback/recovery authority already exists. Without that authority, use `handoff_ready` and name the owner, missing authority, and proof still needed.
12. If proof passes, route to `slice-op-exec-result-writer`. This skill may say proof supports the next result step; it must not write the result truth or claim `executed_with_declared_proof_level` itself.
13. If the operation scope changed into code/config implementation plus live operation, route to `slice.hybrid-implementation-operation`. If active incident scope dominates, route to the incident or specialized operations owner.
14. Write `post-action-validation.md` and stop. Do not continue into observation, rollback/recovery, result writing, promotion, cleanup, or another operational action in the same skill invocation.

## Outputs

Create or update only `post-action-validation.md`.

Minimum output shape:
- `source_inputs`: files, timestamps, evidence paths, and proof contract used;
- `action_reference`: action-ledger row, execution-log entry, actor, timestamp, command/tool summary, and checkpoint status;
- `target_identity`: exact authorized target plus any mismatch;
- `proof_contract`: required, waived, blocked, and unsafe proof classes;
- `evidence_table`: source, timestamp or block/receipt id, target identity, observed value, expected value, verdict, and retained proof path;
- `freshness_verdict`: fresh, stale, cached, pre-action, observation-window-bound, or unknown;
- `validation_verdict`: `post_action_validation_recorded`, `blocked_missing_post_action_proof`, `observation_window_required`, `wrong_target`, `stale_proof`, `failed_proof`, `unsafe_to_collect`, or `handoff_ready`;
- `terminal_state`: `ready_for_next_step` or `stop_or_handoff`;
- `next_route`: `slice-op-exec-result-writer`, `slice-op-exec-observation-window-manager`, `slice-op-exec-rollback-or-recovery-runner`, `slice-op-exec-maintenance-and-handoff`, `slice.hybrid-implementation-operation`, incident/specialized operations owner, or named human/operator.

Use `ready_for_next_step` only when `post_action_validation_recorded` is true. Use `stop_or_handoff` when proof is missing, stale, wrong-target, contradictory, unsafe to collect, blocked by authority, observation-dependent, or failed. The output can support later result classification, but it cannot write `result.md` or upgrade action evidence into live success.

## Verification

Before passing the gate, re-check that `post-action-validation.md` contains no new action command, no rollback command, no deployment instruction, no target mutation, no result truth writing, and no completion wording stronger than the evidence supports. Verify that every required proof class has a row with source, timestamp or block/receipt id, target identity, observed value, expected value, verdict, and proof path or explicit waiver.

Trigger verification must select this skill only after an action attempt and before result writing. It must reject preparation-only, pre-action execution, retry, rollback/recovery, long observation-window, already-validated result-writing, promotion, cleanup, and completion-claim scenarios.

Run:
- `node tools/validate-internal-skill-body-quality.mjs --skill slice-op-exec-post-action-validator`
- `node tools/validate-internal-skill-trigger-fixtures.mjs --skill slice-op-exec-post-action-validator`

Manual validation must confirm exactly seven H2 sections, the owning manifest step, Part 6B, final map `#s6`, final map `#s19`, exact atom row, source_inputs, proof gates, output shape, terminal states, handoff routes, forbidden actions, JSON fixture parsing, whitespace/final newline hygiene, and scoped `git diff --check`.

## Failure Modes

Stop or hand off when the proof contract is missing, the action was not actually attempted, the target identity is ambiguous, evidence targets the wrong service or environment, evidence is stale or cached, live reads are unavailable, a longer observation window is required, proof contradicts expected state, stop conditions are observed, retained evidence would expose secrets, or the requested terminal claim overstates proof.

Route missing action evidence to `slice-op-exec-action-runner` or a named operator. Route delayed proof to `slice-op-exec-observation-window-manager`. Route failed proof or stop-condition recovery to `slice-op-exec-rollback-or-recovery-runner` only with recorded rollback/recovery authority. Route absent authority, external access, unsupported tooling, or manual-only proof to `slice-op-exec-maintenance-and-handoff` or `handoff_ready`. Route changed code-plus-live scope to the hybrid variant and active incident dominance to the incident/specialized operations owner.

Use `blocked_missing_post_action_proof` when the action is logged but no acceptable current evidence can be collected. That zero-proof fallback prevents later result writers from converting an attempted action into a verified live outcome.
