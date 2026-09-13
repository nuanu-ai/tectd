---
id: "slice-debug-entry-gate"
kind: "internal_step"
owner_family: "slice_variant"
pipeline_id: "slice.debug-root-cause"
step_id: "slice-debug-entry-gate"
entry_gate: true
public_trigger: false
native_skill_discovery: false
body_path: "capabilities/instructions/pipelines/slice-variants/debug-root-cause/slice-debug-entry-gate.step.md"
source_manifest: "capabilities/pipelines/slice-variants/debug-root-cause.pipeline.json"
legacy_skill_ref: "slice-debug-entry-gate"
architecture_refs:
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#debug-root-cause-variant"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19"
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#lightweight-and-debug-slice-variants"
---

# Slice Debug Entry Gate

## Overview
This skill is the direct entry gate for the `slice.debug-root-cause` variant. Its core rule is to admit only actual debug work: an observed behavior conflicts with expected behavior, root cause is not established, and the next lawful step is context loading rather than diagnosis, fixing, verification, result writing, promotion, deployment, or live operation.

## When to Use
Use this after Kernel and Runtime have routed a candidate Slice toward `tect-work` and the Slice family needs a debug/root-cause entry verdict. It fits bugs, regressions, failed verification, unexpected runtime behavior, flaky behavior, performance surprises, build failures, integration failures, or contradictory evidence where the cause is unknown.

Use it to classify the symptom, consequence, freshness, authority, and route fit: debug/root-cause versus lightweight known fix, full design work, operational incident, hybrid deploy/live work, current-state query, research, procedure capture, maintenance, setup, or adoption.

Do not use it to investigate evidence, load context, reproduce, trace data flow, form hypotheses, decide root cause, design a fix, write tests, patch source, verify, write results, promote durable knowledge, deploy, recover a live incident, or treat a symptom-level workaround as the root cause.

## Source Contract
Ground this direct step in:

- `capabilities/pipelines/slice-variants/debug-root-cause.pipeline.json#step_graph.steps.slice-debug-entry-gate`
- `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#debug-root-cause-variant`
- `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#lightweight-and-debug-slice-variants`
- `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6`
- `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19`
- `docs/architecture/master-plugin-target-architecture-capability-atom-shard-map.html#pipeline.slice.debug_root_cause.entry.gate`

The manifest step is required, invokes `slice-debug-entry-gate`, produces `slice.md`, gates on `selected_debug_variant` and `authority_declared`, reaches `ready_for_context_loading`, and fails through `route_to_incident_ops_or_block`. The atom row says this gate classifies symptom, consequence, freshness, authority, and debug versus lightweight fix or incident. No external or custom skill body is canonical for this entry gate; `systematic-debugging` is relevant background for later debug steps, not permission for this gate to investigate.

Intake source inputs are limited to the user symptom or failing proof, expected behavior, observed behavior, affected surface, parent Scope or Slice candidate, authority state, freshness/source basis, and available evidence handles: logs, tests, diffs, or runtime state. This gate may classify whether those handles exist and whether they are fresh enough to admit the Slice. It must not read through them as diagnosis, reproduction, evidence ordering, or root-cause work.

## Operating Procedure
Run these sweeps in order and record the result in the entry packet: symptom-fit sweep, root-cause-known sweep, route-fit sweep, consequence/urgency sweep, freshness sweep, authority sweep, and handoff-routing sweep.

1. Confirm there is a concrete failure signal. Require an observed-versus-expected delta, affected surface, and current request context. If the request is only a feature idea, broad design question, current-state query, or operational command, route away before admitting debug.
2. Confirm root cause is unknown. If the cause is already proven and the task is a small code/config change, route to lightweight TDD or the appropriate downstream fix step. If the cause is architectural or cross-component, route to full design or architecture discussion instead of pretending this is entry diagnosis.
3. Classify consequence and urgency. Separate local bug work from live incident, production recovery, deploy-authority-dominant, security, data-loss, or customer-impacting operation. When live response or operational authority dominates, use `route_to_incident_ops_or_block`.
4. Classify freshness. Record whether the failure signal is current, stale, memory-derived, pasted, historical, intermittent, environment-dependent, or contradicted. Stale or missing context can still enter debug only when the next owner is explicitly the context loader; unsupported certainty blocks entry.
5. Declare authority. State read, log, test, local command, source mutation, deploy, live-system, and promotion authority as known, missing, or out of scope. This gate can admit read/context loading; it cannot grant mutation, deployment, live operation, result, or promotion authority.
6. Reject wrong routes explicitly. Name the better owner when the work is a known small fix, full design, ops prep, ops execution, hybrid implementation plus live proof, research, procedure capture, maintenance, setup/adoption, privacy-restricted query, or already past entry.
7. Activate only the debug entry packet. On a valid fit, prepare or update the `slice.md` entry fields: selected debug variant, source input checklist, symptom summary, unknown-root-cause statement, consequence class, freshness basis, authority declaration, rejected routes, handoff routing note, gate status, and next step `slice-debug-context-loader`.
8. Emit the verdict. Use `ready_for_context_loading` only when `selected_debug_variant` and `authority_declared` are both explicit. Otherwise use `route_to_incident_ops_or_block` or `blocked_entry` with the exact missing symptom, authority, freshness, route-fit, or handoff owner condition.

## Outputs
Produce only the debug entry packet for `slice.md`. It must include the observed-versus-expected delta, affected surface, source input checklist, root-cause-unknown status, consequence and urgency class, source freshness label, authority declaration, selected variant, rejected alternative routes, handoff routing note, gate results, terminal verdict, and next owner.

Allowed successful terminal state is `ready_for_context_loading`. Failure must use `route_to_incident_ops_or_block` or a `blocked_entry` note that preserves the exact missing condition and routes the next owner for incident/ops, human input, authority declaration, context refresh, or another Slice variant. This skill must not create `symptom.md`, `reproduction.md`, `evidence-log.md`, `hypotheses.md`, `root-cause.md`, `fix-plan.md`, `verification.md`, `result.md`, `promotion.md`, `handoff.md`, patches, deployments, or live-operation records.

## Verification
Verify trigger fit by checking that positive scenarios contain an actual observed behavior conflict with unknown cause and need a debug entry verdict before context loading. Verify negative scenarios route away when the cause is known, the work is a small clear change, the request is broad design, the dominant need is ops or live incident response, or a later debug step already owns the next action.

Run `node tools/validate-internal-skill-body-quality.mjs --skill slice-debug-entry-gate` and `node tools/validate-internal-skill-trigger-fixtures.mjs --skill slice-debug-entry-gate`. Also parse both fixture JSON files, scan headings for exactly `Overview`, `When to Use`, `Source Contract`, `Operating Procedure`, `Outputs`, `Verification`, and `Failure Modes`, run a scoped diff check for the owned files, and scan those files for trailing whitespace.

Content review must confirm the manifest path, Part 6B anchors, final map `#s6` and `#s19`, and atom row are present; the body admits only root-cause/debug entry; and no wording authorizes investigation, fix execution, verification, result writing, promotion, deployment, live-system commands, branch operations, or symptom-as-root-cause shortcuts.

## Failure Modes
Use `route_to_incident_ops_or_block` when the symptom is tied to active user impact, production recovery, operational execution, deploy authority, rollback, security exposure, data loss, credentials, or live-system command needs. The debug entry gate must not normalize incident or ops work into ordinary local debugging.

Block or route away when there is no observed-versus-expected delta, root cause is already known, authority is undeclared, the failure report is too stale to admit without refresh, the Slice target is bundled or vague, parent routing is missing, handoff owner is unclear, or the request starts at investigation, patching, verification, result, promotion, deployment, or handoff.

Do not upgrade a symptom, workaround, log message, failing assertion, or suspected file into root cause. Do not continue after repeated failed fixes as if this gate can repair the architecture; route to architecture discussion or full design when the evidence says the problem is structural.
