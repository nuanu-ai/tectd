---
id: "slice-op-exec-maintenance-and-handoff"
kind: "internal_step"
owner_family: "slice_variant"
pipeline_id: "slice.operational-execution"
step_id: "slice-op-exec-maintenance-and-handoff"
entry_gate: false
public_trigger: false
native_skill_discovery: false
body_path: "capabilities/instructions/pipelines/slice-variants/operational-execution/slice-op-exec-maintenance-and-handoff.step.md"
source_manifest: "capabilities/pipelines/slice-variants/operational-execution.pipeline.json"
legacy_skill_ref: "slice-op-exec-maintenance-and-handoff"
architecture_refs:
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#operational-preparation-and-operational-execution-variants"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19"
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#operational-and-hybrid-slice-variants"
---

# Operational Execution Maintenance And Handoff

## Overview
This skill closes the maintenance and handoff edge of a `slice.operational-execution` path. It records what still needs attention after execution, validation, result, recovery notes, and promotion routing have run; it does not perform cleanup, promotion, rollback, further action execution, or completion upgrades.

## When to Use
Use this when Runtime selected `slice.operational-execution`, the active step is `slice-op-exec-maintenance-and-handoff`, and the Slice already has enough closure state to decide whether monitoring, manual verification, recovery follow-up, artifact/index repair, cleanup, deferred work, or user/team handoff remains.

Use it after `result.md`, `recovery-notes.md`, `post-action-validation.md`, optional `observation-window.md`, and optional `promotion.md` or `deferred.md` have been read or explicitly recorded as missing. Do not use it for preparation-only work, command planning, action running, checkpoint verification, post-action validation, rollback or recovery execution, result writing, promotion routing, durable-domain mutation, or a claim that the operation is complete.

## Source Contract
Ground this skill in `capabilities/pipelines/slice-variants/operational-execution.pipeline.json` step `slice-op-exec-maintenance-and-handoff`, under `pipeline.slice.operational-execution`. The step produces only `handoff.md`, gates on `maintenance_or_handoff_recorded`, fails by `stop_or_handoff`, and reaches `ready_for_next_step` only after a maintenance request or handoff record exists.

Architecture sources are `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#operational-preparation-and-operational-execution-variants`, `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#operational-and-hybrid-slice-variants`, `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6`, and `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19`. The maintenance contract names `result-presence-check`, `variant-shape-check`, `promotion-readiness-check`, `proof-validation-check`, and `front-door-sync`. Durable-domain storage and maintenance algorithms are outside this step.

## Operating Procedure
1. Load the active Slice identity, selected variant, authority-confirmation, current-state, preflight, risk-stop-conditions, action-ledger, execution-log, post-action-validation, observation-window, recovery-notes, result, promotion, deferred, and prior handoff artifacts when present. Record missing required inputs as gaps instead of inferring them.
2. Name the highest validated truth from `result.md` without upgrading it: executed with declared proof, live verified, rolled back, stopped by stop condition, blocked missing authority, blocked missing preflight, blocked missing post-action proof, or handoff ready.
3. Build the maintenance checkpoint table. For `result-presence-check`, `variant-shape-check`, `promotion-readiness-check`, `proof-validation-check`, and `front-door-sync`, record one status: passed, requested, blocked, stale, not applicable to the recorded truth, or needs owner review. A requested check is not a completed repair.
4. Identify unresolved observations: open monitoring windows, missing manual verification, post-action proof gaps, stale service signals, recovery symptoms to watch, unconfirmed rollback state, or user/team confirmation still required. Tie each item to its source artifact and expected return evidence.
5. Identify manual owner actions. Name the owner, target, exact next action, authority needed, expected evidence to paste back, stale-after condition, and safe pause point. Manual actions may include verifying a dashboard, checking a service, approving rollback, running a user-owned command, or confirming business impact; this skill does not perform them.
6. Identify artifact, index, and cleanup needs separately. Record missing or stale artifacts, generated projection/index/front-door sync needs, follow-up Slice candidates, branch/worktree/log cleanup candidates, and unmanaged operation artifacts. Route repair and cleanup through the proper maintenance, git/worktree, or authority-gated workflow; do not clean, delete, merge, promote, or mutate here.
7. Keep durable promotion separate. If `promotion.md` or `deferred.md` is missing or blocked, record the gap and next owner. If durable KB, runbook, DevOps, security, protocol, operations, product research, or skill-authoring promotion is needed, hand it to the promotion or durable-domain pipeline instead of writing durable storage.
8. Shape `handoff.md` with Slice path, target identity, highest validated truth, proof and freshness refs, unresolved observations, maintenance checkpoint table, manual owner actions, artifact/index repair needs, cleanup candidates, promotion/deferred routing, forbidden claims, resume trigger, and next manifest owner. For a Result with standalone `Outcome: partial`, omit `Resume step:` until the default authority-confirmation gate is freshly verified. After a gate's proof and freshness checks pass, write exactly one standalone `Resume step: <step-id>` naming an existing step in this operational manifest. This pointer advances navigation only; every selected step still enforces its own authority, preflight, stop, and proof gates. Never infer the pointer from prose, duplicate it, or rewrite the partial Result to advance.
9. Return `ready_for_next_step` only when `handoff.md` records maintenance state or explicit handoff state with a named next owner. Return `stop_or_handoff` when highest truth is missing, the target actor is unknown, proof is stale, authority is ambiguous, cleanup would be destructive, promotion would mutate durable storage, or unresolved observations have no owner.

## Maintenance And Handoff Gates
The gate passes only when `handoff.md` records these items or marks each one unavailable with a blocker:

- cleanup and retention requests for logs, evidence, screenshots, generated projections, temporary files, branches, and worktrees;
- follow-up Slice candidates, ownership gaps, user/team handoff requirements, and exact resume triggers;
- observation window owner, watched signal, check time, expected return evidence, and stale-after rule;
- deferred risks, residual recovery symptoms, and unsafe states that must not be hidden behind completion language;
- runbook/procedure candidates and durable-domain promotion candidates routed as pending work, not performed writes;
- maintenance checks for result presence, variant shape, proof validation, promotion readiness, and front-door or index sync.

The terminal state is `ready_for_next_step` only after those gates have named owners and return evidence. Otherwise keep `stop_or_handoff` and route to the manifest owner that can produce missing proof, execute authorized recovery, classify promotion, perform maintenance repair, or request user/team action.

## Outputs
The only output is `handoff.md`. It must include current Slice identity, operational target, terminal or blocked state from `result.md`, proof links, freshness limits, maintenance checkpoint statuses, unresolved observations, manual owner actions, artifact/index repair requests, cleanup candidates, promotion/deferred route, expected return evidence, stale-after rule, resume step, and blocked or forbidden claims. A partial continuation may contain at most one exact `Resume step:` declaration. An absent declaration returns to authority confirmation; an invalid, unknown, duplicate, or conflicting declaration blocks as invalid evidence without closing the Slice.

The output may recommend maintenance checks, repair proposals, cleanup candidates, user commands, durable promotion handoff, or follow-up Slice candidates, but each must be labeled as pending or externally owned. It must not write `result.md`, run cleanup, execute commands, perform rollback, update indexes, create durable-domain records, mark promotion complete, or claim the operation is complete beyond the proof already recorded.

## Verification
Before returning, verify that `handoff.md` references the selected `slice.operational-execution` path, uses the exact manifest step `slice-op-exec-maintenance-and-handoff`, and records the gate `maintenance_or_handoff_recorded`. Verify all five maintenance checkpoints are present or explicitly marked unavailable, every unresolved observation has an owner or blocker, and every manual action has expected return evidence. If `Resume step:` is present, verify it occurs exactly once, names an existing operational step, and is supported by fresh completion of the preceding gate while `result.md` preserves the original partial truth.

Content verification must show no hidden completion: local proof is not live proof, a requested maintenance check is not a repair, a cleanup candidate is not cleanup performed, a promotion candidate is not durable promotion, and a user/team handoff is not agent completion. Validate the skill body and trigger fixture with `node tools/validate-internal-skill-body-quality.mjs --skill slice-op-exec-maintenance-and-handoff` and `node tools/validate-internal-skill-trigger-fixtures.mjs --skill slice-op-exec-maintenance-and-handoff`.

## Failure Modes
Stop or hand off when `result.md` is missing, highest validated truth is unclear, post-action proof is absent or stale, live proof is required but not present, recovery state contradicts the result, maintenance checkpoint inputs cannot be read, manual owner is unknown, artifact/index repair would require source mutation, cleanup would delete or merge without authority, or durable promotion is being requested as a side effect.

Route back to result writing when closure truth is missing, post-action validation when proof is incomplete, observation management when monitoring is still active, rollback or recovery when authorized recovery must run, promotion routing when durable follow-up has not been classified, and maintenance repair when an index, projection, artifact shape, or result-presence check reports a repair need. Never hide blocked, stale, partial, or manual-owner states behind a completion claim.
