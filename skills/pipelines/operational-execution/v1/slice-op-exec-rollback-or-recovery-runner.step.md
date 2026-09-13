---
id: "slice-op-exec-rollback-or-recovery-runner"
kind: "internal_step"
owner_family: "slice_variant"
pipeline_id: "slice.operational-execution"
step_id: "slice-op-exec-rollback-or-recovery-runner"
entry_gate: false
public_trigger: false
native_skill_discovery: false
body_path: "capabilities/instructions/pipelines/slice-variants/operational-execution/slice-op-exec-rollback-or-recovery-runner.step.md"
source_manifest: "capabilities/pipelines/slice-variants/operational-execution.pipeline.json"
legacy_skill_ref: "slice-op-exec-rollback-or-recovery-runner"
architecture_refs:
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#operational-and-hybrid-slice-variants"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19"
---

# Operational Execution Rollback Or Recovery Runner

## Overview

This skill owns the rollback or recovery decision point inside `slice.operational-execution`. It runs only after an authorized operation has failed, partially completed, failed a checkpoint, failed post-action validation, or crossed a declared stop condition.

Core rule: rollback or recovery is a proof-backed authority path, not cleanup, not incident concealment, and not completion. The skill may run a bounded rollback or recovery action only when current proof, exact procedure, target boundary, command boundary, and explicit rollback or recovery authority all exist. Otherwise it records a blocked recovery handoff.

## When to Use

Use this when the active manifest step is `slice-op-exec-rollback-or-recovery-runner` in `capabilities/pipelines/slice-variants/operational-execution.pipeline.json`, or when current operational execution evidence shows a failed command, partial target state, checkpoint failure, post-action validation failure, unsafe observed state, or stop condition that requires rollback/recovery routing.

Do not use this for initial execution, preflight, routine checkpoint verification, routine post-action validation, observation-only monitoring, preparation-only rollback planning, result writing, promotion, or root-cause investigation before a failed execution signal exists. If live incident scope dominates generic operational execution, stop and route to incident or specialized ops rather than treating it as routine recovery.

## Source Contract

Architecture sources are `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html` section 9, `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6`, `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19`, and `docs/architecture/master-plugin-target-architecture-capability-atom-shard-map.html` row `pipeline.slice.operational_execution.rollback.or.recovery.runner`.

Registry source is `capabilities/registry/internal-skill-resolutions.json` record `tect-skill.slice-ops-hybrid.slice-op-exec-rollback-or-recovery-runner`. The owning manifest step is `capabilities/pipelines/slice-variants/operational-execution.pipeline.json#step_graph.steps.slice-op-exec-rollback-or-recovery-runner`: it invokes this skill, produces `rollback.md` and `recovery-notes.md`, gates on `rollback_or_recovery_authority_checked`, fails by `stop_or_handoff`, and may continue as `ready_for_next_step`.

Authority boundary: this skill does not create authority. It consumes explicit runtime/user authority for rollback or recovery, separate from original execution authority, and rejects any action outside the approved target, actor, operation class, command/tool list, environment, timeout, and stop conditions.

Source inputs: `authority-confirmation.md`, `current-state.md`, `preflight.md`, `risk-stop-conditions.md`, `action-ledger.md`, latest `execution-log.md`, latest `post-action-validation.md`, prior `rollback.md` or `recovery-notes.md` when present, user approval text, and fresh live/current proof for the affected target.

## Operating Procedure

1. Confirm the trigger and active step. Verify that current evidence proves failed, partially completed, stopped, or unsafe operational execution. If no recovery trigger exists, route back to checkpoint verification, post-action validation, observation, or result writing.
2. Load and reconcile source inputs. Mark missing, stale, contradictory, wrong-target, or wrong-environment evidence before any action. Do not reconstruct procedure from memory or chat summary when source artifacts are missing.
3. Run rollback vs recovery decision checks. Choose no-op with proof when the target is already safe; choose rollback when a declared path can restore the last known good version, config, data state, route, flag, or release; choose forward recovery when rollback is unavailable, unsafe, insufficient, or a bounded mitigation is safer; choose manual handoff when authority or access is missing; choose incident/specialized ops when blast radius or urgency exceeds this Slice.
4. Check authority before action. Confirm separate rollback or recovery authority names the target, actor, permitted action class, exact command/tool boundary, environment, approval source, proof requirement, and stop conditions. If authority is absent, stale, ambiguous, or narrower than the proposed action, stop with blocked recovery handoff.
5. Re-check gates: `rollback_or_recovery_authority_checked`, fresh current proof, preflight relevance, stop-condition status, action-ledger consistency, command boundary, credential safety, worker collision risk, and incident escalation threshold. Any failed gate routes to `stop_or_handoff`.
6. Bound one action at a time. Before invocation, record command/tool, cwd or target, environment assumptions, timeout, expected output, proof check, abort condition, and evidence destination. Do not run destructive, cleanup, deployment, migration, data, chain, credential-affecting, or broad service actions unless that exact action is authorized.
7. If runtime and user authority permit tool use, run only the next bounded action and record input, output, exit status, timestamp, deviation, observation, and immediate proof. If tool use is not authorized, write the exact user/team handoff instead of simulating success.
8. Verify after each action against the proof contract and stop conditions. Command exit alone is not proof. If proof is incomplete, record partial/unknown state; do not claim recovered, rolled back, healthy, complete, or safe.
9. Write artifacts and route. `ready_for_next_step` is allowed only when authority was checked and recovery evidence is sufficient for the next validator/result step. Otherwise use `stop_or_handoff` with exact owner, missing proof, blocked action, and next check.

## Outputs

`rollback.md` records trigger, baseline, authority source, selected rollback posture, rollback-versus-recovery rationale, bounded action list, allowed commands/tools, actual commands/tools invoked, outputs, observations, proof checks, skipped actions, and whether rollback was performed, not required, not applicable, unsafe, blocked, or handed off.

`recovery-notes.md` records current validated state, recovery class, residual risk, partial/unknown status, observation window, follow-up checks, manual continuation owner, evidence locations, incident/escalation route, and forbidden claims. If no action was taken, it must still explain whether recovery was unnecessary, unauthorized, unsafe, outside the command boundary, or waiting for user/team action.

Forbidden actions: no improvised rollback commands, no rollback without explicit authority, no recovery without current proof, no command execution outside approved boundary, no post-action proof bypass, no completion claim from command exit alone, no credential or secret dump, no durable knowledge/runbook promotion, no result truth rewrite, and no routine handling of incident scope.

Terminal states: this skill emits `ready_for_next_step` only after `rollback_or_recovery_authority_checked` and artifact-backed evidence exist. It emits `stop_or_handoff` for missing authority, missing proof, unsafe action, stale source truth, incident escalation, manual owner requirement, or ongoing observation. Result classification such as `rolled_back_verified`, `stopped_by_stop_condition`, or `blocked_missing_authority` belongs to the downstream result writer and must not be overclaimed here.

Handoff and next-step routing: route to `slice-op-exec-post-action-validator` when proof must be re-run, `slice-op-exec-result-writer` when highest truth is ready to classify, `slice-op-exec-recovery-notes-writer` when residual recovery detail remains, `slice-op-exec-maintenance-and-handoff` when manual action or observation remains, `slice.debug-root-cause` when cause is unknown, `slice.hybrid-implementation-operation` when code/config change is required, and incident/specialized ops when live impact exceeds this Slice.

## Verification

Verify source fidelity by checking the architecture and manifest anchors in `Source Contract`, and by confirming the manifest output shape remains `rollback.md` plus `recovery-notes.md`.

Verify behavior by proving: the recovery trigger is real; rollback/recovery authority is explicit and separate from original execution authority; source inputs are fresh enough for the target; rollback vs forward recovery was selected by safety and feasibility; every action stayed inside the approved command boundary; current proof exists before and after any action; and terminal routing is either `ready_for_next_step` with evidence or `stop_or_handoff` with exact missing owner/proof.

Run:
- `node tools/validate-internal-skill-body-quality.mjs --skill slice-op-exec-rollback-or-recovery-runner`
- `node tools/validate-internal-skill-trigger-fixtures.mjs --skill slice-op-exec-rollback-or-recovery-runner`

## Failure Modes

Block or hand off when rollback/recovery authority is missing, stale, ambiguous, broader than the command boundary, narrower than the proposed action, contradicted by current state, or not granted for the current actor/session.

Block when proof is stale, current state is unknown, the action ledger is incomplete, preflight assumptions no longer hold, a stop condition worsened, target/environment identity is ambiguous, another worker may own the live surface, credentials or secrets would be exposed, action would destroy unrecoverable data outside approval, or live incident scope requires a specialized path.

Never make a silent recovery claim. If proof cannot be gathered, preserve negative evidence, record partial/unknown state, identify the next owner, and route to validation, result writing, recovery notes, manual handoff, debug, hybrid, or incident escalation without hiding the failed or partial operation.
