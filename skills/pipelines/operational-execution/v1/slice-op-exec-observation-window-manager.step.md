---
id: "slice-op-exec-observation-window-manager"
kind: "internal_step"
owner_family: "slice_variant"
pipeline_id: "slice.operational-execution"
step_id: "slice-op-exec-observation-window-manager"
entry_gate: false
public_trigger: false
native_skill_discovery: false
body_path: "capabilities/instructions/pipelines/slice-variants/operational-execution/slice-op-exec-observation-window-manager.step.md"
source_manifest: "capabilities/pipelines/slice-variants/operational-execution.pipeline.json"
legacy_skill_ref: "slice-op-exec-observation-window-manager"
architecture_refs:
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#operational-preparation-and-operational-execution-variants"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19"
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#operational-and-hybrid-slice-variants"
---

# Operational Execution Observation Window Manager

## Overview
This skill manages the time-boxed observation window after an operational action when immediate post-action validation is insufficient. Its core rule: no operational-execution Slice may close, promote, or claim completion while the observation gate lacks a current `pass`, `fail`, `inconclusive`, or `not_required` verdict.

## When to Use
Use this after `slice-op-exec-post-action-validator` or `slice-op-exec-checkpoint-verifier` identifies delayed truth: propagation, queues, background jobs, retries, health stabilization, chain finality, user-visible behavior, log quiet periods, SLO/error-rate watch, or manual confirmation that needs elapsed time.

Use it only inside selected `slice.operational-execution` work with explicit authority, current state, preflight, risk-stop conditions, action ledger, execution log, and post-action validation context already present.

Do not use it to perform the original action, invent new verification scope, run unrelated checks, execute rollback, write the final result, or replace immediate post-action proof. If no elapsed observation is required, record why the observation gate is not required and route forward.

## Source Contract
Ground this skill in `capabilities/pipelines/slice-variants/operational-execution.pipeline.json`, step `slice-op-exec-observation-window-manager`. The manifest marks the step required, produces `observation-window.md`, gates on `observation_recorded_or_not_required`, fails by `stop_or_handoff`, and reaches `ready_for_next_step` only after the observation gate is resolved.

Architecture source truth is `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#operational-preparation-and-operational-execution-variants`, where operational execution requires explicit authority, action proof, stop conditions, optional `observation-window.md`, and no completion claim without post-action proof. Also preserve `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#operational-and-hybrid-slice-variants`, `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6`, and `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19`.

Atom anchors are `pipeline.slice.operational-execution`, `pipeline.slice.operational_execution.observation.window.manager`, and `capabilities/pipelines/slice-variants/operational-execution.pipeline.json#step_graph.steps.slice-op-exec-observation-window-manager`.

Source inputs are the selected Slice context, `authority-confirmation.md`, `current-state.md`, `preflight.md`, `risk-stop-conditions.md`, `action-ledger.md`, `execution-log.md`, `post-action-validation.md`, proof contract, checkpoint verdicts, and any explicitly approved handoff notes naming delayed truth.

## Operating Procedure
1. Decide whether observation is required. Use the post-action proof contract, checkpoint verdict, risk-stop conditions, action ledger, and expected delayed effects. If every required proof is already current and no delayed signal is declared, record `not_required` with evidence and route forward.
2. Define the watch set before observing. Name each signal, source, owner, acceptable threshold, failure threshold, inconclusive threshold, freshness limit, and evidence location. Typical signals include service health, error rate, logs, queue depth, job completion, API smoke, release/config state, DB or chain event, balance/state transition, UI behavior, and user or operator confirmation.
3. Set the observation window. Record start time, duration, cadence, max missed samples, sample method, clock basis, stop-on-fail rules, and the accountable owner for each sample: agent, user, on-call owner, team, or external system. Cadence must be tight enough to catch declared stop conditions but bounded to the approved target and proof contract.
4. Keep scope read-only and relevant. Use only approved observation sources from the Slice artifacts or explicit handoff. Do not mutate target state, broaden target identity, add unrelated diagnostics, hide credentials, or treat silence as success when the source contract expected an observable signal.
5. Evaluate each sample against thresholds and freshness. A sample is usable only when its timestamp is inside the freshness limit, the source identity matches the target, and the value can be compared with the declared threshold. Mark stale, missing, contradictory, or off-target evidence separately.
6. Emit a window verdict. Use `pass` when all required signals stay within acceptable thresholds through the full duration. Use `fail` when any failure threshold or stop condition triggers. Use `inconclusive` when evidence is missing, stale, off-target, manually blocked, or the window expires before truth is knowable.
7. Route the next step. `pass` may satisfy `observation_recorded_or_not_required` and route to result writing. `fail` pauses continuation and routes to rollback/recovery when authority exists, or to recovery notes and handoff when it does not. `inconclusive` pauses completion and names the next owner, missing evidence, extended-window request, or manual verification needed.

## Outputs
Primary output is `observation-window.md` in the selected Slice folder when observation is required, or an explicit not-required observation gate entry when it is not.

Use this output shape: observation decision, source inputs reviewed, target identity, watched signals, signal owners, source locations, duration, cadence, start/end timestamps, freshness rules, acceptable/failure/inconclusive thresholds, samples or evidence pointers, missed samples, stop-condition review, pause/rollback/recovery triggers, handoff owner, final verdict, and whether `ready_for_next_step` or `stop_or_handoff` applies.

This skill must not write `result.md`, execute rollback, rerun the operation, or claim final completion. It supplies the observation outcome required before those later owners can decide highest validated truth.

## Verification
For content validation, confirm this body has exactly the seven Layer 6B H2 sections, references the operational-execution manifest, names the Part 6B operational anchors plus `#s6` and `#s19`, and describes observation-window behavior rather than wrapper routing.

For runtime use, inspect the proposed `observation-window.md`: every watched signal has an owner, source, duration, cadence, threshold, freshness limit, evidence pointer, and verdict; no unrelated checks or mutations were added; fail and inconclusive outcomes pause completion; rollback/recovery or handoff is routed by authority and stop-condition evidence; and no completion claim is made without an observation outcome.

Run targeted validators after edits:

- `node tools/validate-internal-skill-body-quality.mjs --skill slice-op-exec-observation-window-manager`
- `node tools/validate-internal-skill-trigger-fixtures.mjs --skill slice-op-exec-observation-window-manager`

## Failure Modes
Stop or hand off when the watch set is undefined, signal ownership is missing, duration or cadence is absent, thresholds are vague, evidence is stale or off-target, the target identity changed, a failure threshold or stop condition triggers, observation requires authority not granted, rollback/recovery authority is absent, an incident owner supersedes the Slice, or the operator cannot provide required manual confirmation.

Also stop when the agent tries to mutate state, run unrelated diagnostics, extend monitoring without an owner, treat missing evidence as success, collapse `inconclusive` into `pass`, or write the final result before the observation gate has a current `pass`, `fail`, `inconclusive`, or `not_required` outcome.

Keep zero-continuation posture until that outcome exists.
