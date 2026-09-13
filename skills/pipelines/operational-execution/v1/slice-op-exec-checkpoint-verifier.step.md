---
id: "slice-op-exec-checkpoint-verifier"
kind: "internal_step"
owner_family: "slice_variant"
pipeline_id: "slice.operational-execution"
step_id: "slice-op-exec-checkpoint-verifier"
entry_gate: false
public_trigger: false
native_skill_discovery: false
body_path: "capabilities/instructions/pipelines/slice-variants/operational-execution/slice-op-exec-checkpoint-verifier.step.md"
source_manifest: "capabilities/pipelines/slice-variants/operational-execution.pipeline.json"
legacy_skill_ref: "slice-op-exec-checkpoint-verifier"
architecture_refs:
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#operational-preparation-and-operational-execution-variants"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19"
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#operational-and-hybrid-slice-variants"
---

# Operational Execution Checkpoint Verifier

## Overview
This skill verifies checkpoint truth during an Operational Execution Slice. Its core rule: an action command is only evidence that the action was attempted; continuation requires expected-vs-observed state proof, stop-gate review, and an explicit checkpoint verdict.

This is a read-only checkpoint verifier for target systems. It may inspect already supplied artifacts, logs, pasted outputs, and evidence files, and it may write or propose the checkpoint entry required by `execution-log.md` when artifact-write authority already exists. It must not execute commands, run fresh probes, retry actions, mutate target systems, alter `action-ledger.md`, rewrite `current-state.md`, weaken stop conditions, execute rollback, or hide a failed checkpoint.

## When to Use
Use this after `slice-op-exec-action-runner` has recorded one or more risky action steps and before the next mutating step, rollback/recovery step, post-action validation, observation window, result, or promotion decision depends on those steps.

Use it when the Slice has authority, baseline, preflight, risk-stop conditions, action ledger, and execution-log evidence, but still needs a checkpoint to decide whether observed state matches the expected intermediate state.

Do not use it to run the original action, create the command ledger, perform final post-action validation, manage a long observation window, execute rollback, or claim the operation completed. Route those cases to their owning operational-execution steps.

## Source Contract
Ground this skill in `capabilities/pipelines/slice-variants/operational-execution.pipeline.json`, step `slice-op-exec-checkpoint-verifier`. The manifest requires this step, produces `execution-log.md`, gates on `checkpoints_verified_or_stopped`, and stops or hands off on failure.

Architecture source truth is `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#operational-preparation-and-operational-execution-variants`, which defines operational execution as authorized side-effect work with authority, preflight, rollback posture, action ledger, stop conditions, and live/post-action proof. Also use `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#operational-and-hybrid-slice-variants` to preserve the shared no-invisible-completion rule, `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6` for the Slice proof/result boundary, and `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19` for runtime variant selection.

The atom anchor is `pipeline.slice.operational_execution.checkpoint.verifier`; the registry also preserves `pipeline.slice.operational-execution` and `capabilities/pipelines/slice-variants/operational-execution.pipeline.json#step_graph.steps.slice-op-exec-checkpoint-verifier`.

## Source Inputs
Required inputs are `slice.md`, `operation-intent.md`, `authority-confirmation.md`, `current-state.md`, `preflight.md`, `risk-stop-conditions.md`, `action-ledger.md`, and the existing `execution-log.md` command entry for the action being checked. Use optional `source-operation-plan.md`, `dry-run.md`, `rollback.md`, `observation-window.md`, `evidence/`, `logs/`, `screenshots/`, `handoff.md`, or user-provided output only when they already exist and are relevant to the checkpoint.

The checkpoint evidence packet must identify the command-ledger row, action id, command/tool id, cwd or target, timestamp, exit status, stdout/stderr or artifact pointer, expected intermediate state, observed state source, freshness limit, declared stop condition, rollback/recovery threshold, and next step that would run if the checkpoint passes. If any required source is missing, stale, contradictory, or not authorized for inspection, the verifier records `stop_or_handoff`; it does not create substitute proof by running commands.

## Operating Procedure
1. Confirm the checkpoint target. Identify the action-ledger row and `execution-log.md` command entry being checked, including command/tool id, cwd, timestamp, exit status, stdout/stderr or artifact pointer, expected intermediate state, timeout, and declared stop condition.
2. Check ledger and baseline integrity. The action must match `authority-confirmation.md`, `current-state.md`, `preflight.md`, `risk-stop-conditions.md`, and `action-ledger.md`; any unapproved target, changed cwd, missing timestamp, unknown exit status, skipped preflight, stale baseline, or altered stop condition blocks continuation.
3. Load the pre/action/post checkpoint set. Pre means baseline, preflight, authority, risk-stop conditions, and rollback or recovery posture. Action means the executed command evidence and any deviation from the ledger. Post means the already available read-only observation, status output, log/event, version, data, API, UI, chain, or user-confirmed signal expected for this checkpoint.
4. Separate execution evidence from proof. An exit status of 0, successful deploy trace, completed migration command, or "no error" log is not enough. Require an observed state source that can be compared with the expected state for this checkpoint.
5. Compare expected vs observed state. Mark each checkpoint as `matched`, `mismatched`, `missing`, `stale`, `inconclusive`, or `not_yet_observable`. Include the source path, command output, timestamp, freshness limit, and any deviation from expected value, count, version, health, balance, event, or behavior.
6. Re-evaluate stop gates before allowing continuation. Stop when a declared stop condition triggered, evidence is absent or stale, the observed state contradicts expected state, the action exceeded authority or target boundary, rollback authority is needed but absent, or incident/live ownership overrides the Slice.
7. Decide rollback, recovery, observation, or human handoff routing. If observed state is harmful or violates a stop condition, route to `slice-op-exec-rollback-or-recovery-runner` only when rollback/recovery authority and procedure exist; otherwise produce `blocked_recovery_handoff`. If expected state needs time to emerge, route to `slice-op-exec-observation-window-manager` with watched signals, duration, polling cadence, owner, and acceptable/failed thresholds.
8. Append or propose the checkpoint verdict for `execution-log.md`. Use `ready_for_next_step` only when all required checkpoint evidence is current, expected state matches observed state, no stop condition triggered, no authority boundary was exceeded, and no observation window is required before the next action. Otherwise use `stop_or_handoff` and name the blocking evidence, gate, rollback/recovery trigger, observation requirement, or handoff owner.

## Outputs
Primary output: an `execution-log.md` checkpoint entry tied to a specific action-ledger row and command evidence.

The entry must include checkpoint id, action id, ledger row, command/tool id, target or cwd, command exit status, expected state, observed state, evidence source, freshness/timestamp, stop-gate review, rollback or recovery trigger status, observation-window requirement, deviations, terminal decision, and next owner. It may record `ready_for_next_step`, `stop_or_handoff`, `rollback_or_recovery_required`, `blocked_recovery_handoff`, or `observation_window_required` as checkpoint verdict labels.

The `checkpoints_verified_or_stopped` gate is satisfied only by one of two explicit outcomes: every required checkpoint is current and matched, allowing `ready_for_next_step`; or a failed, missing, stale, inconclusive, harmful, or not-yet-observable checkpoint is visible as `stop_or_handoff` with a concrete route. A failed checkpoint must remain visible in the log and handoff; do not relabel it as success because a command exited 0 or because the next action is convenient.

This skill must not create final `post-action-validation.md`, `result.md`, promotion records, rollback output, or operation-completed claims. It only verifies whether the current execution checkpoint permits the next manifest step.

## Verification
For content validation, confirm the body keeps the required Layer 6B sections, references the operational-execution manifest, names the Part 6B operational anchors, includes `#s6` and `#s19`, and describes checkpoint behavior rather than wrapper routing.

For runtime use, inspect the proposed `execution-log.md` entry: every checked action has command evidence, expected state, observed state, source freshness, and stop-gate disposition; action execution is not treated as proof; mismatches route to stop, rollback/recovery, observation, or handoff; and continuation is only allowed when `checkpoints_verified_or_stopped` is satisfied.

Also verify the negative boundary: the checkpoint verifier did not run commands, collect new live proof, retry the action, mutate the target, edit the command ledger/current-state/stop-condition sources, execute rollback, write final result truth, or conceal a failed checkpoint behind a passing terminal state.

Run the targeted validators after edits:

- `node tools/validate-internal-skill-body-quality.mjs --skill slice-op-exec-checkpoint-verifier`
- `node tools/validate-internal-skill-trigger-fixtures.mjs --skill slice-op-exec-checkpoint-verifier`

## Failure Modes
Stop or hand off when command evidence is missing, exit status is unknown, baseline or preflight evidence is stale, `action-ledger.md` no longer matches the executed step, `current-state.md` is too stale for comparison, `risk-stop-conditions.md` is absent or triggered, the observed state cannot be compared to the expected state, rollback/recovery authority is absent, the action touched an unapproved target, credentials or secret handling are unclear, the verifier would need to run a new command to know the truth, or the system needs a time-boxed observation window before truth can be known; use a zero-continuation posture until the blocking gate is cleared.

Route final proof to `slice-op-exec-post-action-validator`, monitoring to `slice-op-exec-observation-window-manager`, rollback execution to `slice-op-exec-rollback-or-recovery-runner`, and result truth to `slice-op-exec-result-writer`. Never let a checkpoint verdict hide an incident, skip required live proof, or convert "command ran" into "operation succeeded."
