---
id: "tect-maintenance-check-requester"
kind: "internal_step"
owner_family: "slice_variant"
pipeline_id: "slice.full-design-to-execution"
step_id: "tect-maintenance-check-requester"
entry_gate: false
public_trigger: false
native_skill_discovery: false
body_path: "capabilities/instructions/pipelines/slice-variants/full-design-to-execution/tect-maintenance-check-requester.step.md"
source_manifest: "capabilities/pipelines/slice-variants/full-design-to-execution.pipeline.json"
legacy_skill_ref: "tect-maintenance-check-requester"
architecture_refs:
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#full-development-slice-variant"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19"
---

# Tect Maintenance Check Requester

## Overview
This skill prepares the maintenance checkpoint request for the full design-to-execution Slice variant. Its core rule is request and route only: name the checks needed to protect Slice closure, including artifact shape, stale projection, result presence, promotion readiness, validation/deployment proof shape, and repair proposal routing. Record what each check must answer and keep completion blocked when proof, source truth, freshness, authority, or repair ownership is missing.

## When to Use
Use when the active lifecycle target is a full design-to-execution Slice or Result and the full variant has reached the maintenance gate after design/spec, decision, plan, execution, verification, deployment-validation, result, promotion, deferred-work, or handoff artifacts have been produced or are expected.

Select this skill when the Slice needs named checkpoint requests for variant shape, required artifact presence, result presence, promotion readiness, front-door/index consistency, stale projections, stale claims, or result/promotion consistency before closure can be trusted. Do not use it for lightweight TDD, debug/root-cause, operational, hybrid, research, procedure-capture, durable-domain-only, adoption, package, team, live-validation, proof-audit, direct repair, cleanup, or durable promotion work.

## Source Contract
Ground this skill in `capabilities/pipelines/slice-variants/full-design-to-execution.pipeline.json#step_graph.steps.slice-maintenance-check-requester`, which invokes `tect-maintenance-check-requester`, gates on `maintenance_gate`, and may end in `maintenance_requested` or `blocked_maintenance_gap`.

Architecture grounding is `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#full-development-slice-variant`, where full development spans design/spec through proof, result, promotion, deferred work, and handoff; `docs/architecture/master-plugin-target-architecture-part-6e-maintenance-capabilities.html`, where maintenance detects drift and proposes repairs without silently mutating source; `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6`, where Result is highest validated truth; `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19`, where Runtime materializes variant steps and maintenance checks; and `docs/architecture/master-plugin-target-architecture-capability-atom-shard-map.html#pipeline.slice.full_design_to_execution.maintenance.check.requester`.

The manifest checkpoint set is `variant-shape-check`, `result-presence-check`, `promotion-readiness-check`, `front-door-sync`, and `stale-projection-check`. External reference bodies: none.

## Operating Procedure
1. Confirm the target is the full design-to-execution Slice maintenance gate. Load the Slice identity, parent Scope, selected full variant, current lifecycle state, authority state, freshness state, runtime view, proof contract, and requested closure or handoff posture.
2. Build the source basis for the request from the full variant artifact contract: `design-spec.md`, `decisions/`, `cross-cutting-review.md`, `implementation-ready-spec.md`, `implementation-plan.md`, `execution.md`, `verification.md`, `deployment-validation.md`, `result.md`, `promotion.md`, `deferred.md`, and any allowed handoff, evidence, log, screenshot, patch, maintenance, or review support artifacts.
3. Create one checkpoint request per manifest checkpoint. For each request, name the target object or artifact, trigger reason, evidence already consulted, missing or stale inputs, expected verdict shape, blocking effect, owner route, and the downstream consumer that needs the verdict.
4. Map checkpoints to closure risks. Use variant shape for required artifact shape, missing, extra, wrong-variant, or hidden second-lifecycle artifacts; result presence for absent, partial, stale, wrong-target, or false-complete result records; promotion readiness for durable-candidate fit and authority; front-door sync for stale orientation and index/update signals; stale projection for generated views, runtime views, handoffs, or installed copies that may mislead current claims; and validation/deployment proof shape for mismatched local, deploy, live, handoff, blocked, rollback, or missing-proof expectations.
5. Fan in any existing maintenance verdicts without upgrading truth. Accept only source-backed and fresh verdicts; mark missing, stale, contradictory, authority-blocked, repair-required, or owner-needed verdicts explicitly.
6. Decide the maintenance posture: `maintenance_requested` when checks are queued with clear owners; `blocked_maintenance_gap` when closure depends on missing, stale, contradictory, or unauthorized evidence; handoff required when another owner must run checks or approve repair; or follow-up Slice required when the target changed materially.
7. Preserve the boundary in the packet. This skill may request checks, summarize available verdicts, and route repair proposals, but it must not run maintenance, validate proof, mutate canonical source, regenerate projections, rebuild indexes, deploy, perform live checks, delete artifacts, promote durable knowledge, or declare the Slice complete.

## Outputs
Return a full-Slice `maintenance_checkpoint_request` or equivalent handoff packet. Include the Slice target, parent Scope, selected full variant, closure or handoff claim under review, checkpoint list, source paths consulted, stale or missing inputs, existing verdicts, blocking effects, owner routes, repair proposal routing, expected downstream consumers, terminal-state implication, and forbidden claims.

When some verdicts are already available, include a fan-in summary grouped as current, missing, stale, contradictory, blocked by authority, repair-required, promotion-candidate, or no-check-needed. The packet may feed `result.md`, `promotion.md`, `deferred.md`, `handoff.md`, transition records, repair proposals, or a follow-up Slice recommendation. It is not itself a maintenance verdict, proof verdict, source repair, durable write, deploy result, cleanup result, or closure record.

## Verification
Verify the request cites `capabilities/pipelines/slice-variants/full-design-to-execution.pipeline.json`, at least one `docs/architecture/*.html` source, the full variant artifact contract, all five manifest checkpoints, validation/deployment proof shape, and repair proposal routing. Check that each requested checkpoint has a concrete trigger reason, evidence basis, expected verdict, owner route, blocking effect, and downstream consumer.

Run `node tools/validate-internal-skill-body-quality.mjs --skill tect-maintenance-check-requester` for body quality and `node tools/validate-internal-skill-trigger-fixtures.mjs --skill tect-maintenance-check-requester` for trigger coverage. A valid trigger set selects this skill for full design-to-execution maintenance request assembly and rejects generic maintenance execution, proof audit, result writing, promotion target selection, direct repair, source mutation, projection regeneration, deploy/live validation, cleanup, and durable-domain promotion.

## Failure Modes
Use `blocked_maintenance_gap` when the Slice identity, parent Scope, selected full variant, runtime view, proof contract, authority state, freshness state, or source artifact basis cannot be loaded. Block closure when result presence, variant shape, stale projection, front-door sync, or promotion readiness cannot be checked with fresh source-backed evidence.

Route to a repair proposal when source artifacts, generated projections, front doors, result records, deferred registers, or promotion edges contradict each other. Route to proof validation when the issue is whether evidence proves a claim. Route to result writing when the result artifact itself must be authored. Route to promotion or durable-domain owners when durable knowledge movement is requested. Route to deployment or live-validation owners when the missing evidence is deploy or live proof.

When the routed owner is `repair-proposal-builder`, treat it only as the next owner workflow. This skill must not execute that builder, accept the proposal, or mark a repair proposal as completed.

Stop with a boundary note if asked to apply source edits, rebuild an index, regenerate a projection, delete or archive artifacts, validate proof, deploy, run a live command, promote durable knowledge, or claim the Slice complete from this requester alone. Preserve the unresolved checkpoint and owner route so Runtime can resume from an explicit blocker instead of false completion.

The zero-loss fallback is to keep every unresolved checkpoint visible, keep stale inputs labeled as stale, and leave the full Slice blocked, handed off, or routed to owner workflow until fresh evidence returns.
