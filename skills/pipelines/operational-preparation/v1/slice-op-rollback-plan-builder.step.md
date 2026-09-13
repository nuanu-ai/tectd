---
id: "slice-op-rollback-plan-builder"
kind: "internal_step"
owner_family: "slice_variant"
pipeline_id: "slice.operational-preparation"
step_id: "slice-op-rollback-plan-builder"
entry_gate: false
public_trigger: false
native_skill_discovery: false
body_path: "capabilities/instructions/pipelines/slice-variants/operational-preparation/slice-op-rollback-plan-builder.step.md"
source_manifest: "capabilities/pipelines/slice-variants/operational-preparation.pipeline.json"
legacy_skill_ref: "slice-op-rollback-plan-builder"
architecture_refs:
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#operational-preparation-and-operational-execution-variants"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19"
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#operational-and-hybrid-slice-variants"
---

# Rollback Plan Builder

## Overview

Build a preparation-only rollback plan for an operation package. The plan names what would be reverted, who owns the decision, what thresholds trigger rollback or manual recovery, and what proof would show that the rollback worked.

This skill is an artifact builder, not an executor. It preserves `prepared_not_executed` truth, writes the planned `rollback-plan.md` content, and must not run commands, mutate target systems, approve operations, execute rollback, or claim recovery proof.

## When to Use

Use this when `slice.operational-preparation` has an operation target, desired final state, authority boundary, current-state baseline, risk-impact notes, preflight checks, and an operation plan, and now needs `rollback-plan.md`.

Trigger on deploy, seed, migration, config, data, infrastructure, chain, or service operations where another actor may later execute the plan and needs a prepared rollback path. The request usually asks what to revert, when to stop, who owns rollback, what evidence to collect, when rollback is unsafe, or how to hand off a recovery path before any mutation is authorized.

Do not use this after execution has begun, when rollback must actually be run, when the user is asking for live recovery, when root cause is still unknown, when code/config edits are needed first, or when rollback authority is missing and the only honest output is a blocker. Route those cases instead of stretching this preparation skill.

## Source Contract

- Architecture: `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#operational-preparation-and-operational-execution-variants` defines operational preparation as exact commands, checklist, preflight, rollback, proof, and handoff without target mutation.
- Architecture: `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6` sets operational preparation artifacts to operation target, authority, rollback, checklist, and handoff with ready/blocked/handoff completion.
- Architecture: `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19` selects operational prep for deploy/seed/redeploy preparation when the ops target has no execution authority.
- Manifest: `capabilities/pipelines/slice-variants/operational-preparation.pipeline.json` step `slice-op-rollback-plan-builder` produces `rollback-plan.md`, gates on `rollback_or_recovery_path_declared`, and fails by recording a rollback gap or block.
- Atom anchors: `pipeline.slice.operational_preparation.op.rollback.plan.builder` and `pipeline.slice.operational-preparation` ground the input/output and variant relationship.
- Artifact contract: `slice-variant-artifact-contract:slice.operational-preparation@0.1.0` requires `rollback-plan.md` in the selected Slice folder and forbids execution-owned outputs such as `execution-log.md`, `action-ledger.md`, `post-action-validation.md`, and `authority-confirmation.md`.
- External references: none were declared for this exact step.

## Operating Procedure

1. Load the prepared operation package inputs: `operation-intent.md`, `authority-boundary.md`, `current-state.md`, `risk-impact.md`, `preflight-checks.md`, and `operation-plan.md`. Extract operation target, desired final state, actor, approval boundary, current state, known risk, planned command sequence, and declared stop conditions. If any source input is absent, record the exact missing source instead of inventing a rollback path.
2. Define the rollback target in concrete terms: service version, package pin, database state, configuration value, chain state, infrastructure resource, generated projection, or manual state that would need reversal.
3. Separate reversible steps from irreversible or partially reversible steps. Mark data loss, external propagation, chain finality, customer-visible downtime, secret exposure, destructive deletes, and third-party side effects as irreversibility blockers or manual recovery cases.
4. Set trigger thresholds before execution: failed preflight, unexpected command output, timeout, health regression, alert threshold, failed proof check, user-visible incident, authority mismatch, or any stop condition from risk-impact notes.
5. Assign the rollback owner and decision boundary. Name whether the user, operator, team, or later execution agent can decide rollback, and state when human approval is required before any rollback command.
6. Prepare rollback commands and checks without running them. Include cwd, environment assumptions, required credentials by name only, command order, expected output, timeout, and the read-only checks that prove rollback readiness. Mark every command as future execution by a later authorized actor.
7. Define rollback proof: status endpoint, logs, DB query, version or config check, smoke test, chain/API evidence, user confirmation, or manual inspection. Tie each proof item to the target state it validates.
8. Add manual handoff instructions for unsafe, unavailable, or authority-blocked rollback. Include who must be contacted, what evidence to provide, what must not be attempted, and where the later actor should record results.
9. State forbidden actions in the artifact: no rollback execution, no target mutation, no operation approval, no deploy/write/delete/seed/migrate action, no live-system command, no secret value capture, no operation-completed claim, and no recovery-proof claim.
10. Close with terminal-state handling: `rollback_plan_ready` when a feasible path and proof exist, `record_rollback_gap_or_block` when rollback is impossible, unsafe, unowned, unproven, or outside preparation authority, and downstream `prepared_not_executed`, `handoff_ready`, `blocked_missing_authority`, `blocked_missing_preflight`, or `escalated_to_operational_execution` only when later steps own that state.

## Outputs

The primary output is `rollback-plan.md`. It must contain this shape:

- rollback target and protected current state;
- intended reversal or recovery path;
- trigger thresholds and stop boundaries;
- owner and approval boundary;
- prepared commands, checks, or manual recovery steps;
- proof required for rollback readiness and for later recovery success;
- manual handoff route for unsafe or blocked rollback;
- irreversibility blockers, residual risk, and explicit terminal verdict.

Adjacent artifacts may be referenced but not rewritten by this skill: `authority-boundary.md`, `current-state.md`, `risk-impact.md`, `preflight-checks.md`, `operation-plan.md`, `proof-contract.md`, `handoff.md`, and `result.md`. The result truth remains `prepared_not_executed`.

This skill must not create or update execution-owned artifacts: `execution-log.md`, `action-ledger.md`, `post-action-validation.md`, `authority-confirmation.md`, live proof, deployment proof, or recovery notes from actual rollback.

## Verification

Verify the body by checking that the rollback plan names a concrete target, at least one trigger threshold, an owner, prepared commands or manual recovery checks, proof required, explicit stop boundaries, handoff/escalation route, and terminal/failure state. Confirm that every rollback command is phrased as prepared-for-later execution, not as already run.

Validate the source contract against `capabilities/pipelines/slice-variants/operational-preparation.pipeline.json`: the step output is `rollback-plan.md`, the gate is `rollback_or_recovery_path_declared`, and the failure route records a rollback gap or block. Also check that no operation-completed, deploy/write/delete/seed/migrate, rollback-executed, target-mutated, approval-granted, or recovery-proof claim appears in preparation output.

Validation commands for this skill body are `node tools/validate-internal-skill-body-quality.mjs --skill slice-op-rollback-plan-builder` and `node tools/validate-internal-skill-trigger-fixtures.mjs --skill slice-op-rollback-plan-builder`, plus JSON parse, heading shape, whitespace, final-newline, and `git diff --check` checks over the owned files.

## Failure Modes

Block or hand off when the current state is unknown, the desired final state is vague, the operation plan is missing, rollback authority is absent, proof cannot be observed, or rollback would be more dangerous than the original operation.

Escalate to `slice.operational-execution` only when explicit execution authority exists and rollback/proof gates are ready. Escalate to `slice.hybrid-implementation-operation` when code or config changes must be implemented before the operation. Escalate to `slice.debug-root-cause` when the failure mode is unknown. Escalate to an incident or specialized ops pipeline when live impact dominates the preparation Slice. Never hide irreversible risk, secret requirements, missing owner, or manual-only recovery behind a ready claim.

If the rollback target depends on a third party, delayed finality, destructive data change, expired backup, unavailable credential, or protected environment, mark the plan blocked until the responsible owner confirms the recovery route and proof window.

If later work needs an actual recovery action, route to `slice-op-exec-rollback-or-recovery-runner` with fresh authority, preflight, live proof, and action logging. Treat any unclear or unsafe recovery area as a hazard until a responsible operator accepts it.
