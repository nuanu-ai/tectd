---
id: "slice-op-user-handoff-package-builder"
kind: "internal_step"
owner_family: "slice_variant"
pipeline_id: "slice.operational-preparation"
step_id: "slice-op-user-handoff-package-builder"
entry_gate: false
public_trigger: false
native_skill_discovery: false
body_path: "capabilities/instructions/pipelines/slice-variants/operational-preparation/slice-op-user-handoff-package-builder.step.md"
source_manifest: "capabilities/pipelines/slice-variants/operational-preparation.pipeline.json"
legacy_skill_ref: "slice-op-user-handoff-package-builder"
architecture_refs:
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#operational-and-hybrid-slice-variants"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19"
---

# Operational Preparation User Handoff Package Builder

## Overview
Create the `handoff.md` package for a preparation-only operation. The core rule is that the package may give exact owner actions and proof expectations, but this skill does not run commands, perform deployments, mutate target state, or claim the operation is complete.

## When to Use
Use this when the selected Slice variant is `slice.operational-preparation` and upstream preparation artifacts are ready enough to hand to a user, team member, or later authorized agent. Typical triggers include requests for exact commands, preflight checklist, rollback path, proof criteria, where to paste results, or a ready/blocked handoff for a deploy, seed, migration, recovery, service change, or other operation that is not authorized for execution now.

Do not use this when authority is already granted to perform the operation, when code/config implementation is still needed, when the cause is unknown and needs debugging first, when required prep artifacts are missing, or when the next step is result closure, promotion routing, maintenance repair, live validation, or rollback execution.

## Source Contract
Ground this behavior in `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#operational-and-hybrid-slice-variants`, `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6`, `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19`, and `capabilities/pipelines/slice-variants/operational-preparation.pipeline.json`.

The manifest step is `step_graph.steps.slice-op-user-handoff-package-builder`: step type `handoff`, required, produces `handoff.md`, gates on `handoff_ready_for_user_or_team`, fails as `block_missing_handoff`, and reaches terminal state `handoff_ready`. No external skill body is a source for this exact step.

## Operating Procedure
1. Confirm the active Slice is preparation-only: `slice.operational-preparation`, terminal truth `prepared_not_executed`, and no current authority to mutate, deploy, seed, migrate, delete, rollback, or validate post-action target state.
2. Inventory required inputs before drafting: `operation-intent.md`, `authority-boundary.md`, `current-state.md`, `risk-impact.md`, `preflight-checks.md`, `operation-plan.md`, `rollback-plan.md`, `proof-contract.md`, and any `dry-run.md`, `evidence/`, `logs/`, `screenshots/`, or `credentials-notes.md` that already exist.
3. Block instead of drafting if the package lacks a target, owner, ordered command/checklist, stop condition, rollback or recovery pointer, proof return path, or reporting location. Record the missing item as the reason for `block_missing_handoff`.
4. Assemble `handoff.md` in owner-action order: context, authority boundary, before-start preflight, exact commands or tool actions the owner must run, expected output for each step, stop conditions, verification proof to collect, rollback or recovery pointer, result-reporting location, and forbidden claims.
5. Keep commands owner-run. Include cwd, environment prerequisites, placeholders for secrets without exposing values, timeout or observation windows when known, and expected output snippets or state changes. Do not add agent instructions to execute them.
6. Add a stale-after timestamp or refresh condition for current-state, credentials, deploy target, source ref, and environment assumptions; if the stale-after point is unknown, block and name the owner who must define it.
7. Tie every command or checklist item to a proof requirement. Name the artifact or message the owner must return: copied terminal output, endpoint response, deploy trace, database query result, service health, screenshot, receipt, log excerpt, or explicit user confirmation.
8. Put stop conditions before irreversible steps. Stop conditions include failed preflight, stale current state, missing credentials, unexpected diff, wrong environment, missing rollback authority, dangerous output, partial action, or any mismatch with the proof contract.
9. Point to rollback or recovery rather than performing it. State who may start rollback, which artifact contains the rollback steps, what evidence proves rollback, and when rollback is unsafe or requires escalation to `slice.operational-execution` or `slice.hybrid-implementation-operation`.
10. End with reporting instructions: where to paste results, which files or fields to update next, who owns the next decision, and whether the follow-up should be result writing, operational execution, hybrid work, debug, promotion, maintenance, or blocked handoff.
11. Label no-execution truth plainly: the package is prepared for owner action, not executed, not live-validated, and not completed.

## Outputs
Primary output is `handoff.md` with an ordered, owner-facing package. It must include operation target, authority owner, authority boundary, preflight checklist, command/checklist sequence, expected outputs, stale-after or refresh requirement, proof collection instructions, stop conditions, safety warnings, rollback or recovery pointer, reporting destination, next actor, and forbidden claims.

The output may reference existing optional evidence folders or dry-run artifacts, but it must not create an execution ledger, action log, post-action validation result, deployment proof, durable runbook promotion, source mutation, or live-system claim. Valid terminal posture is `handoff_ready`; if required handoff material is missing, return `block_missing_handoff`.

## Verification
Check the final package against the manifest gate `handoff_ready_for_user_or_team`: a different owner can read `handoff.md`, perform only the authorized future action, know what to verify, know when to stop, know where rollback guidance lives, and know where to report results.

If any stale-after or refresh condition has passed, mark the package stale and route back to current-state or preflight refresh before owner action.

Verify that `handoff.md` preserves preparation-only truth: no command was run by this skill, no target mutation occurred, no deploy/write/delete/seed/migrate action is claimed, no hidden credential appears, dry-run evidence is not treated as post-action proof, and completion is not claimed. Also verify that all architecture and manifest anchors above remain visible in this skill body and that trigger fixtures cover positive handoff selection and non-trigger execution/debug/result cases.

## Failure Modes
Block with `block_missing_handoff` when the package cannot name the operation owner, target, ordered steps, expected outputs, proof to collect, stop conditions, rollback pointer, or result-reporting destination. Block or route away when authority is ambiguous, current-state evidence is stale, preflight requirements are unknown, rollback is unsafe, proof expectations are missing, or credentials would need to be exposed.

If the package is partial, keep the owner action paused and name the exact missing input, responsible party, and follow-up skill. If a future actor reports new evidence, treat that as a new result or execution path instead of retroactively upgrading this preparation handoff. Use zero-action wording whenever the handoff is blocked or waiting for approval.

Route to `slice.operational-execution` when the user grants explicit authority to perform the operation now. Route to `slice.hybrid-implementation-operation` when code/config changes plus deployment or live proof are needed. Route to debug/root-cause when the operation target or cause is unknown. Route to result writing only after `handoff.md` exists and the highest truth can be recorded as prepared, handed off, blocked, or escalated.
