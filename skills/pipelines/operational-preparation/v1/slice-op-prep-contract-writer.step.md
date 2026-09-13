---
id: "slice-op-prep-contract-writer"
kind: "internal_step"
owner_family: "slice_variant"
pipeline_id: "slice.operational-preparation"
step_id: "slice-op-prep-contract-writer"
entry_gate: false
public_trigger: false
native_skill_discovery: false
body_path: "capabilities/instructions/pipelines/slice-variants/operational-preparation/slice-op-prep-contract-writer.step.md"
source_manifest: "capabilities/pipelines/slice-variants/operational-preparation.pipeline.json"
legacy_skill_ref: "slice-op-prep-contract-writer"
architecture_refs:
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#operational-preparation-and-operational-execution-variants"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19"
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#operational-and-hybrid-slice-variants"
---

# Operational Preparation Contract Writer

## Overview
Create the `slice.md` contract for a preparation-only operational Slice. The core rule is that preparation may package an operation and its proof expectations, but it must not mutate the target, run commands, grant authority, or imply the operation is complete.

This is a reference_adapter_skill for `superpowers:writing-plans`: adapt its concrete task, implementation plan, and verification discipline into an Tect operational-preparation contract. Do not call the external skill as the runtime implementation, copy its body, or turn `slice.md` into a full plan.

## When to Use
Use after operational-preparation variant selection, intent capture, and context loading. The selected variant is `slice.operational-preparation`; `operation-intent.md` either satisfies `target_and_final_state_captured` with the operation target and desired final state or declares the gap; context loading has supplied the source basis or named what is missing; the next artifact is the prep-only `slice.md` contract.

Use it when the user wants exact commands, a checklist, preflight criteria, rollback or recovery posture, proof criteria, dry-run/read-only validation rules, or a user/team handoff package, but execution is not authorized now.

Do not use it to capture initial intent, load context, declare the detailed authority boundary, baseline current state, model risk, build the command plan, write rollback steps, build the proof contract, run dry-runs, package handoff, write result, promote a runbook, or request maintenance checks. Do not use it when execution authority is granted now, when code or config changes are needed, when root cause is unknown, when live incident handling dominates, or when the operation already ran.

## Source Contract
The owning source is `capabilities/pipelines/slice-variants/operational-preparation.pipeline.json`, step `slice-op-prep-contract-writer`. The step is required, invokes `slice-op-prep-contract-writer` and `superpowers:writing-plans` as reference input, produces `slice.md`, gates on `prep_boundary_declared`, fails with `block_missing_contract`, and reaches `contract_ready`.

Architecture grounding: `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#operational-preparation-and-operational-execution-variants`, `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#operational-and-hybrid-slice-variants`, `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6`, and `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19`.

Source inputs for this step are the parent Slice/Scope constraints, variant-selection record, `operation-intent.md`, context-loader output, manifest artifact contract, and any explicit user authority statement. If those inputs conflict, the contract records the conflict and routes rather than guessing.

## Operating Procedure
1. Confirm the Slice is `slice.operational-preparation` and the current step is contract writing. If the selected variant, operation target, desired final state, or context basis is missing, route back to `slice-op-prep-intent-capture` or `slice-op-prep-context-loader` instead of writing an invented contract.
2. Write the prep-only boundary in `slice.md`: selected variant, terminal truth `prepared_not_executed`, allowed preparation actions, forbidden target actions, no command execution, no target mutation, no deploy/write/delete/seed/migrate action, and no operation-completed claim.
3. Copy the operation target and desired final state from intent capture. Name the target exactly enough for later commands to avoid the wrong repo, host, service, chain, database, account, environment, branch, or deployment surface. If current state is unknown, state the gap and hand off to `slice-op-target-state-baseliner`; do not baseline live state here.
4. Record source inputs and freshness: which user statement, runbook, deployment doc, environment map, service state, prior incident, parent constraint, or declared unknown supports the contract. Mark stale or missing source truth as a blocker for later artifacts.
5. Require read/write/execute/deploy separation at contract level. Distinguish read inspection, local preparation, safe dry-run or read-only validation, command execution, remote write, deploy/apply, seed/migrate/delete, rollback/recovery, live validation, durable promotion, and handoff. The contract may require `authority-boundary.md`; it must not grant authority.
6. Instantiate artifact obligations. Required downstream artifacts are `README.md`, `slice.md`, `operation-intent.md`, `authority-boundary.md`, `current-state.md`, `risk-impact.md`, `preflight-checks.md`, `operation-plan.md`, `rollback-plan.md`, `proof-contract.md`, `handoff.md`, and `result.md`. Optional support is `dry-run.md`, `evidence/`, `logs/`, `screenshots/`, `credentials-notes.md`, `promotion.md`, and `deferred.md`. Forbidden prep artifacts are `execution-runs/run-N/`, `execution-log.md`, `action-ledger.md`, `post-action-validation.md`, and `authority-confirmation.md`.
7. Adapt writing-plans discipline without importing its runtime. State that the implementation plan distinguished from slice.md rule applies: `slice.md` records the contract, while `operation-plan.md` later owns the command/task list. Include task list guidance adapted from writing-plans: later plan steps must include exact command or checklist item, cwd or target, required authority, expected output, stop condition, rollback relation, and verification expectation.
8. Define the proof-before-execution sequence. Later artifacts must establish current-state baseline, risk and blast radius, preflight evidence, rollback or recovery posture, command plan, proof contract, post-action proof classes, allowed read-only or dry-run evidence, and handoff before any actor executes. Missing proof blocks completion; local prep evidence is not live proof.
9. Add routing rules. Escalate to operational execution when authority is granted. Escalate to hybrid when code or config changes are needed. Route to debug when unknown root cause dominates. Route to `slice-op-authority-boundary-declarer`, `slice-op-target-state-baseliner`, `slice-op-preflight-check-builder`, `slice-op-command-plan-builder`, `slice-op-proof-contract-builder`, or `slice-op-user-handoff-package-builder` when the contract is ready and those later artifacts are next.
10. Run the contract checklist before ending: target and desired final state present; prep boundary present; allowed and forbidden actions present; authority separation required; required downstream artifacts listed; proof-before-execution sequence declared; terminal and failure states declared; forbidden claims excluded. End with `contract_ready` only when all checklist items are true. Otherwise end with `block_missing_contract` and list the missing fields.

## Outputs
Primary output is `slice.md` only for this step.

The contract must include: Slice identity and parent link; selected variant `slice.operational-preparation`; operation target and desired final state; source inputs and freshness; prep-only boundary; allowed preparation actions; forbidden target actions; read/write/execute/deploy separation requirement; required, optional, and forbidden artifacts; proof-before-execution sequence; escalation and handoff routing; terminal state; missing-field blockers; and explicit forbidden claims.

This step may reference downstream artifacts but does not write `authority-boundary.md`, `current-state.md`, `risk-impact.md`, `preflight-checks.md`, `operation-plan.md`, `rollback-plan.md`, `proof-contract.md`, `handoff.md`, `result.md`, `promotion.md`, or `deferred.md`. `result.md` later states prepared-not-executed truth; this step only makes that truth enforceable.

## Verification
Verify `slice.md` answers: what operation target is being prepared, what desired final state is expected, what source inputs support the contract, what the prep-only boundary is, what actions are allowed, what actions are forbidden, how read/write/execute/deploy authority is separated, which required downstream artifacts must exist, what proof is required before execution, and where each blocker routes.

Check that the contract references the operational-preparation manifest, uses the `prep_boundary_declared` gate, and terminates only as `contract_ready` or `block_missing_contract`. Confirm it does not contain command output, action ledger rows, execution logs, deploy/live proof, rollback proof, hidden credentials, result closure, promotion approval, or operation-completed claims.

Run `node tools/validate-internal-skill-body-quality.mjs --skill slice-op-prep-contract-writer` and `node tools/validate-internal-skill-trigger-fixtures.mjs --skill slice-op-prep-contract-writer`. For fidelity review, also parse both fixtures, scan this file and both fixtures for trailing whitespace, and run scoped `git diff --check --` on the three owned files.

## Failure Modes
Use `block_missing_contract` when the selected variant is not confirmed, target identity is ambiguous, desired final state is not proofable, source inputs are stale or missing, the prep-only boundary is absent, authority separation is unclear, current-state gap is hidden, required artifacts are not declared, proof-before-execution is missing, or terminal routing is uncertain.

Route back to intent capture when target or desired final state is missing. Route back to context loading when the source basis is missing. Route forward to authority boundary, target state baseline, risk, preflight, command plan, rollback, proof, handoff, result, promotion, or maintenance steps only after the contract is ready and the next artifact belongs to that step.

Escalate instead of stretching the skill: operational execution for explicit authority, hybrid implementation plus operation for code/config plus live proof, debug for unknown root cause, incident/specialized ops for live incident ownership, or handoff when the user/team must approve or execute. Never convert preparation into execution by implication.
