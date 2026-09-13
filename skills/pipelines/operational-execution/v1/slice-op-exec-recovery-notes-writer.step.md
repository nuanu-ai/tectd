---
id: "slice-op-exec-recovery-notes-writer"
kind: "internal_step"
owner_family: "slice_variant"
pipeline_id: "slice.operational-execution"
step_id: "slice-op-exec-recovery-notes-writer"
entry_gate: false
public_trigger: false
native_skill_discovery: false
body_path: "capabilities/instructions/pipelines/slice-variants/operational-execution/slice-op-exec-recovery-notes-writer.step.md"
source_manifest: "capabilities/pipelines/slice-variants/operational-execution.pipeline.json"
legacy_skill_ref: "slice-op-exec-recovery-notes-writer"
architecture_refs:
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#operational-preparation-and-operational-execution-variants"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19"
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#operational-and-hybrid-slice-variants"
---

# Operational Execution Recovery Notes Writer

## Overview
This skill records `recovery-notes.md` for the `slice.operational-execution` variant. Its core rule is that recovery notes preserve failure, partial, rollback, blocked, and residual-risk truth so later result and handoff steps cannot hide operational uncertainty.

## When to Use
Use this when the operational execution Slice is active and the current step is `slice-op-exec-recovery-notes-writer` after `execution-log.md`, `post-action-validation.md`, `observation-window.md`, `rollback.md`, or recovery evidence exists or is explicitly missing. Use it for failed or partial execution, stopped-by-stop-condition states, rolled-back or recovery-attempted states, blocked manual recovery, unresolved proof gaps, symptom watch instructions, owner actions, or residual risks before result writing.

Do not use it to run recovery, perform rollback, execute another command, validate post-action proof, write final result, request durable promotion, clean worktrees, or convert an operation into a runbook. If execution has not happened and no failed or blocked recovery context exists, route back to the action, checkpoint, validation, or rollback/recovery owner.

## Source Contract
Ground this skill in `capabilities/pipelines/slice-variants/operational-execution.pipeline.json` step `slice-op-exec-recovery-notes-writer`, under `pipeline.slice.operational-execution`. The manifest step is required, produces `recovery-notes.md`, gates on `recovery_notes_recorded`, falls back by `stop_or_handoff`, and may advance to `ready_for_next_step` only after recovery truth is recorded.

Architecture sources are `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#operational-preparation-and-operational-execution-variants`, `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#operational-and-hybrid-slice-variants`, `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6`, and `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19`. The atom map row `pipeline.slice.operational_execution.recovery.notes.writer` ties this step to recovery notes, residual risk, follow-up checks, later symptom handling, and an approval-required authority boundary. The output belongs to the selected Slice workspace control plane, not durable-domain storage.

## Source Inputs
Required inputs are the selected Slice identity plus available `authority-confirmation.md`, `current-state.md`, `preflight.md`, `risk-stop-conditions.md`, `action-ledger.md`, `execution-log.md`, `post-action-validation.md`, `rollback.md`, `observation-window.md`, checkpoint records, and prior `recovery-notes.md`.

Treat missing artifacts as explicit proof gaps. Do not fill gaps from memory, command exit status, expectation, or user confidence.

## Operating Procedure
1. Load the selected Slice identity and available artifacts: `authority-confirmation.md`, `current-state.md`, `preflight.md`, `risk-stop-conditions.md`, `action-ledger.md`, `execution-log.md`, checkpoint records, `post-action-validation.md`, `rollback.md`, `observation-window.md`, and any prior `recovery-notes.md`. Mark absent required inputs as gaps; do not infer from memory.
2. Classify recovery posture: no recovery needed with residual watch, partial execution, failed action, stopped by stop condition, rollback attempted, rollback verified, recovery attempted, recovery blocked, manual owner action required, or incident/escalation needed.
3. Record authority: who approved the original operation, who approved rollback or recovery if any, what action remains outside agent authority, and which commands or live-system actions must not be performed by this notes step.
4. Capture observed state before and after the failure or recovery decision. Include timestamps when available, exact source artifact references, accepted proof, negative proof, changed or unchanged target state, and freshness limits.
5. Document blocked or manual recovery. Name the owner, requested action, target system, authority needed, expected evidence to return, safe pause point, and stale-after or symptom trigger.
6. Document proof gaps and residual risks separately. Proof gaps are missing evidence needed before result closure; residual risks are known remaining hazards even when proof exists.
7. Record rollback/recovery context without executing it: what was attempted, skipped, blocked, verified, or ruled unsafe; why; and which route owns further action.
8. Write `recovery-notes.md` with source links, recovery posture, authority record, observed state, blocked/manual recovery, proof gaps, residual risks, owner actions, symptom watch, forbidden claims, next route, and terminal recommendation.
9. Return `ready_for_next_step` only when `recovery-notes.md` honestly records the posture and remaining owners. Return `stop_or_handoff` when authority is ambiguous, state evidence conflicts, proof is stale, manual owner is unnamed, recovery is still executing, or notes would imply completion without result proof.

## Proof and Authority Gates
The note must separate proof accepted, proof missing, proof stale, and proof contradicted. Every recovery or rollback statement must name the source artifact and the proof class that supports it.

The note must also name the authority owner for original execution, rollback or recovery approval, manual recovery, and follow-up observation. Missing authority is a blocker, not a reason to execute recovery.

## Terminal States
Use `ready_for_next_step` only when the note records what happened, what was attempted, what proof exists, what proof is missing, who owns remaining recovery or observation, and which claims remain forbidden.

Use `stop_or_handoff` when the recovery owner is unknown, authority is missing, proof conflicts, proof freshness is unknown, manual action is still required, incident scope supersedes the Slice, or any needed action would exceed this notes step.

## Forbidden Actions
Do not execute recovery, run rollback, retry commands, restart services, deploy, clean worktrees, mutate durable knowledge, promote runbooks, validate post-action proof, write `result.md`, or claim the operation completed.

Do not hide failure, partial execution, blocked recovery, missing authority, stale proof, residual risk, or manual handoff behind a softer result label.

## Outputs
The only output is `recovery-notes.md`. It must include the selected `slice.operational-execution` path, manifest step `slice-op-exec-recovery-notes-writer`, gate `recovery_notes_recorded`, operation target, recovery posture, authority basis, observed state, rollback or recovery context, blocked/manual recovery, proof gaps, residual risks, owner actions, symptom triggers, expected return evidence, forbidden claims, next route, and whether the step recommends `ready_for_next_step` or `stop_or_handoff`.

The artifact may recommend a result classification, handoff, observation, rollback/recovery owner, incident escalation, or follow-up Slice. It must not execute hidden recovery, run commands, mutate workspaces, mark rollback complete without proof, upgrade partial proof into live proof, write durable-domain records, or claim the operation is complete.

## Handoff and Next-Step Routing
Route to `slice-op-exec-post-action-validator` when proof is absent but validation can still be gathered without recovery action.

Route to `slice-op-exec-rollback-or-recovery-runner` only when separate authority-gated recovery or rollback execution is required.

Route to `slice-op-exec-observation-window-manager` when delayed symptoms, queue health, logs, or user-visible behavior still need a timed watch.

Route to `slice-op-exec-result-writer` only after recovery truth, proof gaps, residual risks, and remaining owners are recorded without overclaiming.

Route to `slice-op-exec-maintenance-and-handoff`, debug, procedure capture, or incident escalation when manual follow-up, reusable operation capture, deeper root-cause work, or live incident ownership supersedes this Slice step.

## Verification
Before returning, verify `recovery-notes.md` names `capabilities/pipelines/slice-variants/operational-execution.pipeline.json`, `pipeline.slice.operational-execution`, `slice-op-exec-recovery-notes-writer`, and `recovery_notes_recorded`. Verify it distinguishes failure, partial, recovery, rollback, and blocked states; names the authority source; cites observed state evidence; and lists proof gaps and residual risks separately.

Check forbidden claims: no fixed, recovered, rolled back, safe, live, or complete assertion appears unless the cited proof supports that exact state. Validate the skill body and trigger fixture with `node tools/validate-internal-skill-body-quality.mjs --skill slice-op-exec-recovery-notes-writer` and `node tools/validate-internal-skill-trigger-fixtures.mjs --skill slice-op-exec-recovery-notes-writer`.

## Failure Modes
Stop or hand off when selected Slice identity is missing, required artifacts cannot be read, execution state conflicts with validation evidence, rollback/recovery authority is missing, proof freshness is unknown, manual owner is absent, recovery action is still in progress, incident scope takes over, or recording notes would require a command, deploy, rollback, cleanup, durable write, branch/worktree mutation, or live-system action.

Route to post-action validation when proof is missing, rollback/recovery runner when authorized action must still occur, observation-window manager when monitoring has not matured, result writer when recovery truth is recorded, maintenance/handoff when manual follow-up remains, or debug/procedure/escalation when symptoms indicate a deeper incident or reusable operation. Never hide partial, blocked, stale, manual, or risky states behind a completion claim.
