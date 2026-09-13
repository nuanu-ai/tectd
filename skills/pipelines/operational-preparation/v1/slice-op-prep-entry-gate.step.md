---
id: "slice-op-prep-entry-gate"
kind: "internal_step"
owner_family: "slice_variant"
pipeline_id: "slice.operational-preparation"
step_id: "slice-op-prep-entry-gate"
entry_gate: true
public_trigger: false
native_skill_discovery: false
body_path: "capabilities/instructions/pipelines/slice-variants/operational-preparation/slice-op-prep-entry-gate.step.md"
source_manifest: "capabilities/pipelines/slice-variants/operational-preparation.pipeline.json"
legacy_skill_ref: "slice-op-prep-entry-gate"
architecture_refs:
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#operational-preparation-and-operational-execution-variants"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19"
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#operational-and-hybrid-slice-variants"
---

# Operational Preparation Entry Gate

## Overview
This skill is the entry gate for the Operational Preparation Slice variant. Its core rule is to classify the request before planning or touching the target: preparation packages an operation, but it does not execute, deploy, seed, migrate, delete, or claim the operation is complete.

## When to Use
Use this when the user wants exact commands, a checklist, preflight criteria, rollback planning, proof criteria, or a user handoff for an operational target, and execution is not authorized yet.

This is preparation-only admission: accept only when the user wants a safe package, handoff, commands, proof plan, or rollback plan without agent authority to execute the operation.

Do not use it when the user has already granted bounded mutating authority for the operation now; route to operational execution. Do not use it when code or configuration changes are part of making the result real; route to hybrid implementation plus ops. Do not use it for durable-domain work such as canonical knowledge, runbook, security, protocol, product, or operations knowledge maintenance; route to the relevant domain or procedure pipeline. If the root cause is unknown or live incident response dominates, route to debug or incident handling first. If there is no concrete operational target, route out of scope or ask for the missing target.

## Source Contract
This gate is grounded in `capabilities/pipelines/slice-variants/operational-preparation.pipeline.json`, pipeline `slice.operational-preparation`, step `slice-op-prep-entry-gate`, which produces `slice.md`, gates `prep_only_variant_selected` and `authority_boundary_needed`, and reaches `ready_for_intent_capture` or `escalate_or_block`.

Architecture sources: `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#operational-preparation-and-operational-execution-variants`, `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#operational-and-hybrid-slice-variants`, `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6`, and `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19`. Atom and manifest anchors: `pipeline.slice.operational-preparation`, `pipeline.slice.operational_preparation.entry.gate`, and `step_graph.steps.slice-op-prep-entry-gate`.

Allowed source inputs are the user request, parent Slice or Runtime selection packet if available, the operational-preparation manifest entry contract, and stated or read-only current-state evidence. Unknown current state is an input posture to record, not a reason to invent facts or execute checks from this gate.

## Operating Procedure
1. Capture the admission inputs and requested target before planning: user ask, parent object or Runtime packet if present, operation target, desired final state, current-state posture, and stated authority.
2. Classify the candidate as one of six routes: preparation-only, operational execution, hybrid implementation plus ops, debug/incident, procedure or durable-domain route, or out of scope.
3. Require the entry posture before accepting preparation-only: operation target, desired final state, current state or declared unknown, non-goals, target environment, consequence level, and authority posture for read, prep, execute, deploy, write, rollback, and promote.
4. Select preparation-only only when the request is to prepare an exact safe operation package and the allowed authority is read, plan, handoff, or explicitly read-only/dry-run validation later in the pipeline.
5. Escalate to operational execution if the user authorizes mutation now or asks the agent to perform the operation. Escalate to hybrid if implementation, configuration, deploy changes, or live validation are needed for completion. Escalate to debug or incident handling if the root cause is unknown or live response dominates. Escalate to procedure or durable-domain routing if the primary goal is a reusable runbook, canonical knowledge, policy, protocol, security, product, or operations knowledge artifact.
6. Block or ask when target, final state, current-state posture, or authority is missing. Do not fill those gaps with assumptions; record the missing field and the safest next question.
7. Preserve the no-execution boundary in the Slice entry: no mutating command, deploy/write/delete/seed/migrate action, credential dump, rollback action, durable promotion, or operation-completed claim is allowed from this gate.
8. If preparation-only is selected, seed `slice.md` with variant identity, route decision, required next step `slice-op-prep-intent-capture`, known inputs, missing inputs, authority boundary, and terminal state `ready_for_intent_capture`.

## Outputs
The output is an entry decision for `slice.md`: selected route, source inputs used, reason, operation target, desired final state, current-state posture, non-goals, authority posture, forbidden actions, missing fields, and next step. Valid terminal outcomes are `ready_for_intent_capture`, `escalate_or_block`, or a routed alternative such as operational execution, hybrid implementation plus ops, procedure capture, durable-domain routing, debug/incident, or out-of-scope handoff.

This gate does not output operation commands, rollback steps, proof contracts, live validation, or result truth. Those belong to later manifest steps after the prep-only boundary is declared.

## Verification
Verify the trigger by checking that the user request matches preparation-only selection signals from the manifest: exact commands, checklist, preflight, rollback, proof criteria, or handoff without current execution authority. Verify rejection by checking for execution authority, required implementation, unknown root cause, live-incident dominance, or already executed operation.

Verify the body with `node tools/validate-internal-skill-body-quality.mjs --skill slice-op-prep-entry-gate` and trigger cases with `node tools/validate-internal-skill-trigger-fixtures.mjs --skill slice-op-prep-entry-gate`. The final body must keep exactly the required H2 sections, reference the owning manifest and architecture sources, remove wrapper-only boilerplate, and leave no phrase that authorizes target mutation.

## Failure Modes
Block when no target, final state, current-state posture, or authority posture is available. Route away when the correct variant is execution, hybrid, procedure capture, debug, research, query, maintenance, setup, or another non-preparation workflow.

Stop immediately if the request tries to smuggle execution into preparation, such as asking for a "prep" skill to run a deploy, write to production, delete data, seed state, perform rollback, promote a runbook, or declare the operation complete. Preserve the boundary as a blocked or escalated state rather than continuing with an unsafe plan.

Treat ambiguous authority as missing authority. A request that mixes planning language with action verbs must be split into a preparation handoff or escalated to the execution gate after explicit approval.

If classification remains unclear after the minimum missing-field question, keep the Slice unstarted and label the request an operational hazard zone.
