---
id: "slice-procedure-skill-candidate-router"
kind: "internal_step"
owner_family: "slice_variant"
pipeline_id: "slice.custom-procedure-capture"
step_id: "slice-procedure-skill-candidate-router"
entry_gate: false
public_trigger: false
native_skill_discovery: false
body_path: "capabilities/instructions/pipelines/slice-variants/procedure-capture/slice-procedure-skill-candidate-router.step.md"
source_manifest: "capabilities/pipelines/slice-variants/procedure-capture.pipeline.json"
legacy_skill_ref: "slice-procedure-skill-candidate-router"
architecture_refs:
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#hybrid-implementation-operation-and-procedure-capture-variants"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19"
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#procedure-and-research-slice-variants"
---

# Slice Procedure Skill Candidate Router

## Overview

Route a captured procedure candidate toward later skill-authoring intake only when the captured process has enough repeatability, generalization, proof, safety, and reuse evidence to justify skill review. This skill emits a `skill-candidate.md` proposal; it never creates, edits, registers, activates, or promotes a skill.

## When to Use

Use after the Procedure Capture Slice has already produced or supplied the source event, captured steps, normalized procedure, existing-match check, authority-risk assessment, proof contract, secret-safety result, reuse-fit decision, and procedure proposal context.

Use when the candidate is not merely a command recipe or runbook update, and the durable question is whether future agents need a reusable behavior guide, trigger contract, or validation pressure scenarios.

Do not use for one-off work, normal execution of an existing runbook, direct durable KB/runbook edits, capability authoring work that did not come through procedure capture, adoption/import work that belongs to `tect-adopt`, or a user request to activate a skill immediately.

## Source Contract

Grounding sources are `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#hybrid-implementation-operation-and-procedure-capture-variants`, `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#procedure-and-research-slice-variants`, `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6`, and `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19`. These sources define Procedure Capture as proposal-first work: procedure/runbook/skill candidates may be routed, but durable mutation and automatic skill promotion stay behind later approval gates.

The owning manifest step is `capabilities/pipelines/slice-variants/procedure-capture.pipeline.json#step_graph.steps.slice-procedure-skill-candidate-router`. It produces `skill-candidate.md`, must satisfy `skill_candidate_routed_without_activation`, and advances only to `ready_for_next_step` or `stop_or_handoff`.

The atom anchors are `pipeline.slice.procedure-capture` and `pipeline.slice.procedure_capture.skill.candidate.router`. External reference sources for this record are empty.

## Operating Procedure

1. Check the upstream procedure-capture bundle. Require source-event evidence, captured steps, normalized procedure text, existing-match result, authority-risk label, proof contract, secret-safety clearance, reuse-fit decision, generalization notes, and current proposal. If any item is absent, stop or hand off with the missing item named.
2. Classify the durable shape before considering a skill. Exact commands, order-of-operations, rollback steps, and operator checklists are procedure/runbook material. Domain facts, compatibility lessons, or research findings belong to durable KB or a stateful domain owner. A skill candidate is only for repeatable agent behavior: triggers, judgments, safety posture, proof expectations, and validation pressure cases.
3. Apply the five gates: source, proof, safety, generalization, and reuse. The source event must be linked; future runs must have a proof contract; secrets and unsafe operational details must be removed; local host/project facts must be separated from the durable method; and a skill must improve future agent conduct beyond a runbook, command recipe, proof template, or durable note.
4. Pick the narrowest route. Send procedure-shaped material to runbook/procedure update, factual material to durable KB or a domain owner, unclear material to deferred review, unsafe or thin material to blocked handoff, and only mature behavior-shaping process guidance to skill-authoring intake. If the candidate implies import, packaging, staged activation, tutorial, migration, or broader capability adoption, hand off to the appropriate authoring/adoption route instead of expanding this step.
5. Draft `skill-candidate.md` as a proposal. Include source refs, candidate title, intended users, positive triggers, non-triggers, prerequisites, operating behavior, required outputs, safety bounds, proof expectations, validation pressure cases, dependencies, alternatives considered, why this is not just a procedure/runbook update, and the proposed downstream owner.
6. Preserve the proposal-only boundary in the artifact. State that no skill file, manifest step, registry row, activation rule, package metadata, public route, durable KB page, runbook, or canonical workspace truth change is granted by this Procedure Capture step. Include the explicit boundary phrases `no durable KB write` and `no runbook write`.
7. Hand off to the next manifest step with a route decision: `skill_authoring_candidate`, `route_elsewhere`, `rejected_not_now`, or `blocked_missing_evidence`. Use `ready_for_next_step` only when the proposal artifact is complete, redacted, sourced, and non-mutating; otherwise use `stop_or_handoff` with the failed gate named.

## Outputs

Primary output is `skill-candidate.md`. It must be source-linked, redacted, scoped to later authoring/adoption intake, and explicit about why skill review is justified. Use this artifact shape: source refs; candidate title; intended users; positive triggers; non-triggers; prerequisite context; operating behavior; required outputs; safety bounds; proof expectations; validation pressure cases; dependencies; alternatives considered; route decision; downstream owner; and no-activation statement.

Secondary output is the route decision for Result/Promotion or handoff: skill-authoring candidate, route to adoption/authoring intake, route to runbook/procedure, route to durable KB or domain owner, rejected/not-now, or blocked with the missing source, proof, authority, safety, duplicate, or generalization issue. Terminal handling is `ready_for_next_step` for a complete proposal and `stop_or_handoff` for every failed gate or unsafe route.

## Verification

Check that the body keeps exactly this Layer 6B shape, cites architecture and manifest sources, and avoids wrapper-only sections or side-effect authorization. Trigger verification must include at least two positive procedure-capture scenarios and at least one non-trigger.

For content verification, confirm the candidate passes all five gates, contains both trigger and non-trigger guidance, cites the captured procedure evidence, distinguishes procedure/runbook material from skill behavior, names the alternative routes considered, and states that it did not create or activate a skill. The manifest gate is satisfied only when `skill-candidate.md` is a proposal and the next state is `ready_for_next_step` or `stop_or_handoff`.

## Failure Modes

Stop or hand off when source-event evidence, captured steps, reuse-fit proof, secret-safety clearance, or authority-risk classification is absent. Route away from skill authoring when an existing runbook should be updated, the procedure is mostly exact commands, the lesson is domain knowledge, or the workflow is too local to generalize.

Block the route when secrets remain, safety risk is unresolved, future proof cannot be defined, the candidate duplicates an existing durable artifact, or the user asks for immediate skill creation or activation. Never compensate by editing plugin skill source, registries, manifests, public routes, durable KB, runbooks, package files, adoption/import state, promotion records, or workspace truth from this step.
