---
id: "slice-procedure-generalization-shaper"
kind: "internal_step"
owner_family: "slice_variant"
pipeline_id: "slice.custom-procedure-capture"
step_id: "slice-procedure-generalization-shaper"
entry_gate: false
public_trigger: false
native_skill_discovery: false
body_path: "capabilities/instructions/pipelines/slice-variants/procedure-capture/slice-procedure-generalization-shaper.step.md"
source_manifest: "capabilities/pipelines/slice-variants/procedure-capture.pipeline.json"
legacy_skill_ref: "slice-procedure-generalization-shaper"
architecture_refs:
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#hybrid-implementation-operation-and-procedure-capture-variants"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19"
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#procedure-and-research-slice-variants"
---

# Procedure Generalization Shaper

## Overview
This skill turns one captured workflow into a reusable procedure shape without promoting it to durable truth. Its core rule is to separate what must remain invariant from what was only true for the source project, environment, account, tool, or session.

## When to Use
Use this inside the `slice.custom-procedure-capture` variant after `source-event.md` and `captured-steps.md` exist or are explicitly marked insufficient. Select it when the workflow looks reusable but still contains one-off commands, project paths, environment names, credentials, human decisions, timing assumptions, or proof examples that need to become parameters, invariants, preconditions, safe defaults, boundaries, and proof requirements.

Do not use it to follow an existing runbook, execute the captured steps again, inspect duplicate runbooks, scrub secrets, decide reuse fit, write a final runbook, author an active skill, or promote durable knowledge.

## Source Contract
Ground this behavior in `capabilities/pipelines/slice-variants/procedure-capture.pipeline.json` step `slice-procedure-generalization-shaper`, which produces `normalized-procedure.md`, gates `procedure_generalized_without_secret_material`, and can only advance to `ready_for_next_step` or `stop_or_handoff`.

Architecture anchors are `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#hybrid-implementation-operation-and-procedure-capture-variants`, `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#procedure-and-research-slice-variants`, `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6`, `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19`, and `docs/architecture/master-plugin-target-architecture-capability-atom-shard-map.html#pipeline.slice.procedure_capture.generalization.shaper`. Manifest and atom anchors include `pipeline.slice.procedure-capture`, `capabilities/pipelines/slice-variants/procedure-capture.pipeline.json#step_graph.steps.slice-procedure-generalization-shaper`, and `capabilities/pipelines/slice-variants/procedure-capture.pipeline.json#step_graph.steps.slice-procedure-generalization-shaper.invokes.slice-procedure-generalization-shaper`.

## Source Inputs
- `source-event.md`: the observed moment that made the workflow worth capturing.
- `captured-steps.md`: the concrete actions, decisions, checks, failures, and proof seen in the source run.
- Optional supporting evidence: command history, logs, artifacts, screenshots, user approvals, prior Slice links, or durable-domain pointers.
- Explicit insufficiency markers are valid inputs only when they say what is missing and why the step cannot safely infer it.

## Operating Procedure
1. Confirm the input is a procedure-capture candidate, not normal development, debug, research, operation execution, or existing runbook use. If `source-event.md` or `captured-steps.md` is missing and cannot be declared missing with reason, stop with `stop_or_handoff`.
2. Extract the source workflow into a candidate skeleton: goal, actors, ordered phases, decisions, commands or actions, observed checks, recovery moves, final proof, and handoff or closure.
3. Split each source-specific detail into one of three buckets: variable parameter, fixed invariant, or excluded observation. Parameters include repo path, environment, service name, chain, account, provider, command flags, artifact path, approval actor, and evidence location. Invariants are the ordering, preconditions, authority checks, proof-before-claim rules, safety gates, and stop conditions that must survive reuse.
4. Define preconditions and safe defaults before future execution is possible: required inputs, minimum authority, freshness expectations, read-only default posture, dry-run preference, approval triggers, rollback or recovery posture, and when to ask the user instead of continuing.
5. Record forbidden assumptions explicitly. Do not assume the same infrastructure, credentials, branch, service health, permission level, package manager, deployment target, or user intent will exist in the next run. Do not infer proof from the original success unless the captured evidence shows it.
6. Shape variants and boundaries only as proposal information: common case, high-risk case, read-only variant, handoff-only variant, and blocked variant. Keep duplicate checks, secret scrubbing, reuse-fit scoring, proposal writing, skill routing, and promotion for their later manifest steps.
7. Draft `normalized-procedure.md` as a proposal artifact with sections for purpose, parameters, invariants, preconditions, safe defaults, ordered procedure shape, proof requirements, variants, boundaries, unresolved questions, and downstream checks. Mark `ready_for_next_step` only when the procedure shape has no raw secret material and the remaining risks are visible.

## Outputs
The artifact is a proposal file for the active Slice. It captures the reusable form of the observed work while keeping source facts, proof duties, and review limits visible. The file is `normalized-procedure.md`. It should contain a reusable procedure shape, not a final runbook or durable knowledge object. It must preserve source-event provenance, list parameters and invariants, identify preconditions and safe defaults, name forbidden assumptions, specify proof requirements, outline variants, and state boundaries for later duplicate, secret-safety, reuse-fit, proposal, promotion, and result steps.

## Terminal States
- `ready_for_next_step`: `normalized-procedure.md` exists, source provenance is visible, reusable shape is separated from one-off detail, proof requirements are explicit, and raw secret material is absent or clearly blocked for later scrub handling.
- `stop_or_handoff`: required source evidence is missing, the source facts are too thin to generalize, or the reusable shape would require authority, secret handling, duplicate resolution, research, or durable promotion outside this step.

## Forbidden Actions
- Do not execute the captured procedure, rerun commands, call live systems, deploy, mutate a workspace, or change source repos.
- Do not publish a final runbook, write durable KB truth, create an active skill, or update canonical plugin rules.
- Do not complete duplicate checks, secret-safety approval, reuse-fit scoring, proposal writing, promotion, or result closure inside this step.
- Do not treat a successful source run as proof that future executions are safe; only record the future proof requirements.

## Routing
Route to the existing-match checker when the normalized shape may duplicate or update an existing runbook. Route to secret-safety when raw credentials, tokens, private endpoints, or unsafe environment details remain. Route to authority/risk when future execution would need write, execute, deploy, promotion, or approval boundaries. Route to reuse-fit, proposal writer, skill-candidate router, promotion gate, or result writer only after this step has produced a bounded proposal shape. Route away from procedure capture entirely when the user is asking for immediate operation execution, incident repair, research synthesis, or normal development work.

## Verification
Before the step can advance, inspect the proposal artifact against the source event and captured steps. The reader should be able to reuse the shape without relying on hidden session context. Verify `normalized-procedure.md` can be understood without the original session but still cites the source event and captured steps. Check that environment details were parameterized or excluded, invariants are concrete, proof requirements are not replaced by expectation, and no text claims durable write, runbook publication, active skill creation, pipeline execution, deployment, or promotion. The gate `procedure_generalized_without_secret_material` is valid only when raw credentials, tokens, private endpoints, and unsafe environment-specific details are absent or routed to the later secret-safety step as blocked material.

## Failure Modes
Use `stop_or_handoff` when the source event is missing, captured steps are too vague, the workflow depends on unshareable secrets, the reusable core cannot be separated from one environment, the next execution would require authority not represented in the source, or the normalized shape would duplicate an existing runbook without the later match check. Route away when the user wants to execute an operation now, repair current state, research evidence, write a final runbook, promote durable truth, or author an active skill.
