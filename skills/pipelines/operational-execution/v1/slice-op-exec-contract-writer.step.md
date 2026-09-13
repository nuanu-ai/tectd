---
id: "slice-op-exec-contract-writer"
kind: "internal_step"
owner_family: "slice_variant"
pipeline_id: "slice.operational-execution"
step_id: "slice-op-exec-contract-writer"
entry_gate: false
public_trigger: false
native_skill_discovery: false
body_path: "capabilities/instructions/pipelines/slice-variants/operational-execution/slice-op-exec-contract-writer.step.md"
source_manifest: "capabilities/pipelines/slice-variants/operational-execution.pipeline.json"
legacy_skill_ref: "slice-op-exec-contract-writer"
architecture_refs:
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#operational-preparation-and-operational-execution-variants"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19"
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#operational-and-hybrid-slice-variants"
---

# Operational Execution Contract Writer

## Overview
Create the `slice.md` contract for a selected `slice.operational-execution` Slice. The contract makes the execution boundary explicit before any current-state baselining, preflight, command ledger, approval, action, checkpoint, rollback, observation, proof, result, promotion, or handoff step can proceed.

This skill owns contract writing only. It does not approve execution, run commands or tool calls, verify checkpoints, manage observation windows, execute rollback or recovery, validate post-action proof, write results, promote runbooks, or claim completion.

## When to Use
Use this after `slice-op-exec-entry-gate`, `slice-op-exec-authority-confirmation`, and `slice-op-exec-context-loader` have selected operational execution and supplied enough source basis to write `slice.md`. Select it when the next missing gate is `execution_boundary_declared`.

Do not use it for preparation-only requests, authority discovery, current-state capture, preflight execution, final approval, action running, checkpoint verification, rollback or recovery execution, post-action validation, observation-window management, promotion routing, maintenance handoff, or result writing.

Route away immediately when the work is actually operational preparation, hybrid implementation plus operation, debug/root-cause, active incident handling, procedure capture, or a later operational-execution step.

## Source Contract
This skill implements `capabilities/pipelines/slice-variants/operational-execution.pipeline.json` step `slice-op-exec-contract-writer`. The manifest says the step invokes `skill:slice-op-exec-contract-writer`, produces `slice.md`, gates on `execution_boundary_declared`, fails by `stop_or_handoff`, and can only end `ready_for_next_step`.

Architecture grounding: `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#operational-preparation-and-operational-execution-variants`, `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#operational-and-hybrid-slice-variants`, `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6`, `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19`, and `docs/architecture/master-plugin-target-architecture-capability-atom-shard-map.html`.

Atom grounding: `pipeline.slice.operational-execution`, `pipeline.slice.operational_execution.contract.writer`, `capabilities/pipelines/slice-variants/operational-execution.pipeline.json#step_graph.steps.slice-op-exec-contract-writer`, and `capabilities/pipelines/slice-variants/operational-execution.pipeline.json#step_graph.steps.slice-op-exec-contract-writer.invokes.slice-op-exec-contract-writer`.

Source inputs are the runtime variant-selection record, parent Slice or Scope link, `operation-intent.md` or prepared handoff/runbook basis, `authority-confirmation.md`, context-loader output such as `README.md`, operation target, desired final state, credential constraints, known or declared-unknown current state, preflight requirements, stop conditions, post-action proof requirements, and any existing `source-operation-plan.md` or `dry-run.md`.

## Operating Procedure
1. Confirm the active variant is `slice.operational-execution`, the selected target is bounded, and source inputs are available. If variant, target, authority, or context is missing, stop with `stop_or_handoff` and name the missing field.
2. Run the required sweeps before writing: variant fit, target identity, authority posture, source-input provenance, current and target state expectations, proof contract, rollback or recovery posture, stop conditions, observation needs, artifact obligations, and adjacent-step ownership.
3. Write `slice.md` with identity: parent object, selected variant, manifest source, exact target identity, environment, operation intent, desired final state, non-goals, source inputs, and any aliases or targets that are explicitly out of bounds.
4. Write the operation boundary: allowed action classes, forbidden action classes, credential handling limits, mutation surfaces, actor ownership for later steps, and the rule that no action runs until downstream preflight, risk-stop, ledger, and approval gates pass.
5. Bind the authority posture without expanding it. Separate read, write, execute, deploy, promote, rollback, recovery, credential, live-validation, and observation authority; mark partial or unknown authority as blocked for affected next steps.
6. Define current and target state expectations without proving them. Name what is known, what is declared unknown, and what `slice-op-exec-current-state-baseliner` must prove before mutation.
7. Define the proof contract: required preflight evidence, post-action proof classes, live or remote proof freshness, acceptable handoff proof, observation-window requirement, and forbidden completion claims when proof is absent.
8. Define rollback or recovery posture separately from forward execution. State whether rollback is authorized, whether recovery is manual-only, what stop conditions force pause, and what future `recovery-notes.md` must preserve.
9. Instantiate artifact obligations: required `README.md`, required `operation-intent.md`, required `authority-confirmation.md`, required `current-state.md`, required `preflight.md`, required `risk-stop-conditions.md`, required `action-ledger.md`, required `execution-log.md`, required `post-action-validation.md`, required `result.md`, required `recovery-notes.md`, plus required `slice.md`; optional `source-operation-plan.md`, `dry-run.md`, `rollback.md`, `observation-window.md`, `evidence/`, `logs/`, `screenshots/`, `promotion.md`, `deferred.md`, and `handoff.md`; forbidden `execution-runs/run-N/`.
10. End with next-step routing: `ready_for_next_step` routes to `slice-op-exec-current-state-baseliner` and then preflight when `execution_boundary_declared` is true; `stop_or_handoff` routes to authority confirmation, context loading, operational preparation, hybrid, debug, incident/specialized ops, or user handoff when a blocker remains.

## Outputs
Primary output is the active Slice `slice.md`. It must contain selected variant, parent link, source inputs, target identity, operation boundary, authority posture, current and target state expectations, proof contract, rollback or recovery posture, stop conditions, observation needs, artifact obligations, gate `execution_boundary_declared`, terminal states, and next-step routing.

Allowed terminal states for this step are only `ready_for_next_step` and `stop_or_handoff`. The contract may reference later artifacts, but it must not create or fill `current-state.md`, `preflight.md`, `risk-stop-conditions.md`, `action-ledger.md`, `execution-log.md`, `post-action-validation.md`, `result.md`, `recovery-notes.md`, `promotion.md`, or `handoff.md`.

## Verification
Verify `slice.md` names `slice.operational-execution`, cites the owning manifest and architecture source, declares source inputs, exact target identity, operation boundary, authority posture, current and target state expectations, proof contract, rollback or recovery posture, stop conditions, observation needs, required sweeps, artifact obligations, `execution_boundary_declared`, terminal states, and next-step routing.

Check that every adjacent responsibility remains outside this skill: authority approval, current-state proof, preflight execution, command sequencing, final approval, action running, checkpoint verification by the contract writer, observation window management by the contract writer, rollback execution by the contract writer, post-action validation, result writing, recovery-note writing, promotion routing by the contract writer, and completion claims are all forbidden.

Run `node tools/validate-internal-skill-body-quality.mjs --skill slice-op-exec-contract-writer` and `node tools/validate-internal-skill-trigger-fixtures.mjs --skill slice-op-exec-contract-writer`. Confirm the H2 list is exactly Overview, When to Use, Source Contract, Operating Procedure, Outputs, Verification, Failure Modes.

## Failure Modes
Return `stop_or_handoff` when the selected variant is not operational execution, source inputs are missing, target identity is ambiguous, authority posture is absent or broader than `authority-confirmation.md`, current or target state cannot be named, proof requirements are unknown, rollback or recovery ownership is unclear, stop conditions are missing, observation needs are unresolved, required artifacts would be hidden, or `execution-runs/run-N/` is present.

Escalate to operational preparation when execution is not authorized now. Escalate to hybrid implementation plus operation when code or config change is part of the outcome. Escalate to debug when the operation is being used to repair an unknown cause. Escalate to incident or specialized ops handling when live incident ownership overrides the normal Slice. Escalate to the owning downstream operational-execution step when `slice.md` already exists and later artifacts or checks are next.

Never broaden authority, hide credentials, skip preflight, continue after a stop condition, authorize rollback without rollback authority, run commands, create action logs, claim live proof, write the final result, or silently promote an operation into a runbook/procedure.
