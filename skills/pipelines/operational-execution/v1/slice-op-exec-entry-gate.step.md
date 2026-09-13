---
id: "slice-op-exec-entry-gate"
kind: "internal_step"
owner_family: "slice_variant"
pipeline_id: "slice.operational-execution"
step_id: "slice-op-exec-entry-gate"
entry_gate: true
public_trigger: false
native_skill_discovery: false
body_path: "capabilities/instructions/pipelines/slice-variants/operational-execution/slice-op-exec-entry-gate.step.md"
source_manifest: "capabilities/pipelines/slice-variants/operational-execution.pipeline.json"
legacy_skill_ref: "slice-op-exec-entry-gate"
architecture_refs:
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#operational-preparation-and-operational-execution-variants"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19"
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#operational-and-hybrid-slice-variants"
---

# Operational Execution Entry Gate

## Overview

This skill is the entry gate for `slice.operational-execution`. Its core rule is to admit only true operational execution: a bounded operational side effect may happen now, the target boundary is explicit, the authority path is concrete, and rollback, proof, and stop-condition readiness can be evaluated by later gates.

The gate selects, blocks, or hands off. It does not run commands, build a detailed command plan, perform preflight, mutate a workspace or live system, execute rollback, write final results, promote durable knowledge, or declare the operation complete.

## When to Use

Use after Kernel and Runtime have routed the request to `tect-work`, selected a Slice path, and need to decide whether the operational execution variant is allowed to start. Select it for operation-only work such as deploy, redeploy, restart, rollback, recovery, seed, migration, config apply, service change, chain/API action, or live-system mutation when the user has given or is giving explicit authority to act on a bounded target now.

Do not use for operational preparation when the user only wants commands, checklist, preflight, rollback, proof criteria, or handoff without execution. Do not use for hybrid implementation when code, config, schema, or infra changes must be made before the operation. Route unknown root cause to debug; route current-state questions to query; route result writing, promotion, procedure capture, durable-domain grooming, setup/adoption, and maintenance repair to their own owners. Reject vague requests such as "fix production", "handle it", or "run whatever is needed" when target, authority, rollback, proof, or stop conditions are not explicit enough to state.

## Source Contract

Ground this step in `capabilities/pipelines/slice-variants/operational-execution.pipeline.json#step_graph.steps.slice-op-exec-entry-gate`. The step is required, invokes `skill:slice-op-exec-entry-gate`, produces the entry portion of `slice.md`, gates on `execution_variant_selected` and `target_boundary_declared`, fails by `stop_or_handoff`, and advances only as `ready_for_next_step`.

Architecture source truth is `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#operational-preparation-and-operational-execution-variants`, `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#operational-and-hybrid-slice-variants`, `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6`, and `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19`. The atom row is `pipeline.slice.operational_execution.entry.gate` in `docs/architecture/master-plugin-target-architecture-capability-atom-shard-map.html`, with `pipeline.slice.operational-execution` as the owning variant anchor.

The operational execution manifest requires authority confirmation, current state, preflight, risk stop conditions, a later action-ledger artifact, an execution log, post-action validation, result, and recovery notes later in the sequence. This entry gate only decides whether the Slice may enter that sequence and whether ledger preparation is required and feasible for the downstream ledger builder.

## Operating Procedure

1. Confirm the parent work context names a Slice candidate, operation target, desired final state, and selected `slice.operational-execution` possibility. If the parent context is still broad, route back to Slice parent or Scope rather than admitting execution.
2. Classify the operation type and target system. Name the service, environment, repo, database, queue, deployment, chain, API, account, infrastructure resource, or other live target that may change. Reject bundled targets that need separate Slices.
3. Require an explicit mutation or side-effect target. If the request is read-only, planning-only, checklist-only, or handoff-only, route to operational preparation. If source work is required before the operation is real, route to hybrid implementation or development. If the failure cause is unknown, route to debug.
4. Check authority readiness at entry level. Require a concrete authority path for write, execute, deploy, rollback, live validation, or credential use as applicable: granted now, approval-gated with named actor, user-owned, or blocked. Implied permission is not enough for production, destructive, irreversible, credentialed, or high-blast-radius work.
5. Check rollback or recovery posture before admission. Require a named rollback path, recovery owner, accepted no-rollback risk, or blocked recovery note that later risk and final-approval gates can evaluate. Missing rollback posture blocks admission.
6. Check proof and stop-condition readiness. Require at least the proof class that will validate the operation, such as live API, logs, database query, version check, chain event, UI smoke, or user confirmation, plus stop-condition categories that would halt or roll back the operation. Missing proof or stop-condition readiness routes to preparation or handoff.
7. Check action-ledger posture at entry level. For any admitted agent-side side effect, mark an action ledger as required unless the operation is explicitly user-owned handoff. Confirm there is enough source material for the later ledger builder to prepare ordered rows: target identity, actor, authority source, command or tool-call source, proof expectation, timeout or pause rule, stop conditions, and rollback or recovery relation. Do not write ledger rows or consume the ledger.
8. Reject unsafe scope explicitly. Stop when authority is ambiguous, credentials would be exposed, preflight is known to fail, an incident owner should take over, rollback authority is missing for risky work, the action-ledger posture is blocked, or the requested action exceeds the named target.
9. Emit the entry portion of `slice.md`: accepted or blocked verdict, target boundary, operation type, requested side effect, authority path, rollback posture, proof expectation, stop-condition readiness, action-ledger posture, rejected variants, terminal state, and next owner.
10. On success, set `execution_variant_selected`, set `target_boundary_declared`, end as `ready_for_next_step`, and hand off to `slice-op-exec-authority-confirmation`. On failure, end as `stop_or_handoff` with the exact missing target, authority, rollback, proof, stop condition, ledger posture, or routing decision.

## Outputs

Produce only the entry-gate packet for `slice.md`. It must include the parent Slice context, target system and environment, operation type, desired final state, requested side effect, explicit target boundary, authority path status, rollback or recovery posture, proof class, stop-condition readiness, action-ledger posture, rejected alternative variants, gate verdict, terminal state, and next step.

Allowed terminal states are `ready_for_next_step` and `stop_or_handoff`. The output may point to later required artifacts such as `authority-confirmation.md`, `current-state.md`, `preflight.md`, `risk-stop-conditions.md`, `action-ledger.md`, `execution-log.md`, `post-action-validation.md`, `result.md`, and `recovery-notes.md`, but it must not create or complete them.

## Verification

Verify the trigger fit: positive cases have a bounded operational side-effect target plus an explicit authority path and readiness to evaluate rollback, proof, and stop conditions now. Negative cases route away when the work is preparation-only, hybrid implementation, unknown-cause debug, current-state query, result/promotion, procedure capture, durable-domain work, setup/adoption, or maintenance.

Run `node tools/validate-internal-skill-body-quality.mjs --skill slice-op-exec-entry-gate` and `node tools/validate-internal-skill-trigger-fixtures.mjs --skill slice-op-exec-entry-gate`. Also parse both fixture JSON files, scan this body for exactly the seven required H2 sections in order, run a scoped whitespace check on the three owned files, and inspect the diff to confirm no command execution, detailed command plan, ledger-row creation, deployment, rollback, result writing, promotion, durable-domain write, hidden credential, or completion claim is authorized from this gate.

## Failure Modes

Use `stop_or_handoff` when the target system, environment, desired final state, mutation boundary, authority path, rollback posture, proof class, stop conditions, action-ledger posture, credential handling, or parent Slice context is missing or contradictory. Stop when the operation is unsafe to admit because preflight is already failing, stop conditions are already true, the action is irreversible without approval, or incident ownership overrides the generic operational path. Treat any unclear authorization as a block, not as a weaker form of approval.

Route preparation-only work to operational preparation, source-plus-live work to hybrid, unknown-cause recovery to debug, current-state questions to query, post-action truth to result/promotion, reusable process capture to procedure capture, and stale or missing artifact repair to maintenance. Never stretch the entry gate into command planning, action running, rollback, post-action validation, result writing, or durable promotion.

If the user insists on proceeding from a vague or unsafe request, keep the block visible rather than weakening the gate. Name the missing fact or authority, identify the next actor or variant that can resolve it, and preserve the rejected execution reason so later steps cannot treat an unadmitted operation as approved.
