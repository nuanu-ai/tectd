---
id: "slice-lightweight-intent-capture"
kind: "internal_step"
owner_family: "slice_variant"
pipeline_id: "slice.lightweight-tdd-development"
step_id: "slice-lightweight-intent-capture"
entry_gate: false
public_trigger: false
native_skill_discovery: false
body_path: "capabilities/instructions/pipelines/slice-variants/lightweight-tdd/slice-lightweight-intent-capture.step.md"
source_manifest: "capabilities/pipelines/slice-variants/lightweight-tdd.pipeline.json"
legacy_skill_ref: "slice-lightweight-intent-capture"
architecture_refs:
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#lightweight-development"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19"
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#lightweight-and-debug-slice-variants"
---

# Slice Lightweight Intent Capture

## Overview
This skill captures the smallest clear intent contract for a selected Lightweight TDD Slice.
The core rule is clarity before momentum: no lightweight path continues until the user instruction, target boundary, expected behavior, non-goals, consequence, proof need, authority need, and escalation triggers are explicit enough to support focused proof.

## When to Use
Use after `slice-lightweight-entry-gate` has ended in `ready_for_intent_capture` for `slice.lightweight-tdd-development`.
The candidate should already be small, bounded, and selected for lightweight treatment, but it still needs the user intent and acceptance boundary recorded before context loading, workspace preflight, test-target selection, TDD, verification, or result writing.

Use when the next question is what behavior must change, what must not change, who or what the target is, how the user or caller will recognize success, what proof would be enough, and which unclear points must be asked now or escalated.

Do not use for variant admission, broad design, unknown-cause bugs, context loading, workspace preflight, test selection, implementation, verification, deployment/live validation, result writing, promotion, operations, research, current-state lookup, or durable knowledge work.

## Lightweight Boundary Checks
Select lightweight intent capture only when all of these are true:
- The user instruction is narrow enough to express as one target behavior change.
- The affected target is bounded to a known file, component, API, config, copy, or local workflow surface.
- Acceptance can be observed through a focused test, command, inspection, UI/API check, or explicit written proof.
- Consequence, authority, dependency, deploy, and live-system risk are low or can be safely deferred to a later deploy-impact gate.

Route away before capture when another variant owns the work:
- Debug/root-cause: observed behavior is wrong but the cause is unknown or reproduction/evidence is missing.
- Full design-to-execution: architecture, cross-component behavior, data model, API contract, or product decision is unclear.
- Hybrid implementation plus operation: code change and deploy/live operation are both part of the expected success claim.
- Operational preparation or execution: the next useful output is an authority, rollback, dry-run, command, deploy, seed, migration, or live proof plan.
- Research to durable knowledge: the main work is source gathering, synthesis, citation, claim ledger, or promotion/no-promote decision.
- Query/maintenance/procedure: the user asks for current state, drift repair, cleanup, runbook/procedure capture, or stale artifact handling.

## Source Contract
Ground this behavior in `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#lightweight-development`, `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#lightweight-and-debug-slice-variants`, `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6`, and `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19`.

The owning manifest is `capabilities/pipelines/slice-variants/lightweight-tdd.pipeline.json`. The required step is `slice-lightweight-intent-capture`, invoked by `pipeline.slice.lightweight-tdd`, producing `slice.md`, gated by `clear_acceptance_checks`, ending in `intent_captured`, and failing through `ask_user_or_escalate_full`. The relevant atom is `pipeline.slice.lightweight-tdd`; the exact step anchor is `capabilities/pipelines/slice-variants/lightweight-tdd.pipeline.json#step_graph.steps.slice-lightweight-intent-capture`. No external skill body is a source for this skill.

## Operating Procedure
1. Confirm the previous lightweight entry gate already selected the lightweight variant and did not leave unresolved rejection signals such as architecture ambiguity, deploy/live dependency, unknown root cause, missing proof target, or broad component scope.
2. Re-check the boundary against the variant routes above. Keep lightweight only for small, clear, bounded work with tight local proof. Route to debug, full, hybrid, ops, research, query, maintenance, or procedure capture as soon as the trigger better matches those paths.
3. Capture the user instruction in one plain behavior statement: who or what asked, target user or caller, object being changed, desired behavior, current behavior or gap when known, and any exact wording the user made binding.
4. Capture the target boundary: expected files, component, API, command, UI state, config, data shape, or workflow surface in scope; adjacent surfaces out of scope; and any protected behavior that must not change.
5. Capture expected behavior as observable acceptance checks. Each check must be specific enough to become a failing test, command, inspection, UI/API behavior check, or written acceptance proof in a later step.
6. Capture consequence and proof posture: local-only proof, affected proof, deployment proof, live proof, security/approval need, user-visible risk, data risk, team/MR risk, and whether a deploy-impact check may be deferred without weakening the success claim.
7. Capture non-goals, assumptions, and open questions separately. Ask the user when a missing answer changes acceptance, proof, authority, target behavior, deploy/live implications, or consequence. Escalate to full design-to-execution when the missing answer reveals broad design or cross-component uncertainty.
8. Decide the gate. End in `intent_captured` only when acceptance checks are clear enough for the next lightweight context and contract steps. End in `ask_user_or_escalate_full` when acceptance checks are vague, contradictory, unverifiable, dependent on live/deploy proof, authority-sensitive, or too broad for lightweight.
9. Update the intent portion of `slice.md` or return an equivalent handoff payload with the request, expected behavior, target boundary, user instruction, consequence, authority/proof needs, deploy/live implications, non-goals, assumptions, open questions, gate verdict, next step, and terminal state.
10. Stop before loading broad context, selecting tests, editing source, running commands, verifying proof, writing results, promoting knowledge, or performing workspace, branch, deployment, package, maintenance, team, or live-system actions.

## Outputs
Primary output is the intent and acceptance block for `slice.md`.
A successful block records the selected variant, source request, user instruction, target boundary, target behavior, current behavior if known, acceptance checks, consequence, authority/proof needs, deploy/live implications, non-goals, protected surfaces, assumptions, open questions that are safe to defer, gate `clear_acceptance_checks`, terminal state `intent_captured`, and next step.

A failing output records `ask_user_or_escalate_full`, the exact missing or contradictory acceptance issue, the user question or safer target variant, and any proof, authority, source-truth, deploy/live, or scope risk that prevents lightweight continuation. It must preserve uncertainty instead of upgrading it to acceptance.

## Verification
Verify trigger fit by checking that the Slice is already admitted to Lightweight TDD and is specifically at the intent-capture point.
It is too early if the entry gate has not selected lightweight, and too late if context, contract, test target, implementation, verification, result, or promotion work is requested.

Verify content by confirming the output includes request, user instruction, target boundary, expected behavior, non-goals, protected behavior, acceptance checks, consequence, authority/proof needs, deploy/live implications, assumptions or questions, gate verdict, terminal state, and escalation path.
Acceptance checks must be observable, not generic wishes.
The skill body must continue to pass `node tools/validate-internal-skill-body-quality.mjs --skill slice-lightweight-intent-capture` and trigger coverage must pass `node tools/validate-internal-skill-trigger-fixtures.mjs --skill slice-lightweight-intent-capture`.

## Failure Modes
Use `ask_user_or_escalate_full` when the request cannot be turned into observable acceptance checks, when expected behavior conflicts with non-goals, when the affected surface is broader than entry-gate evidence showed, or when proof depends on deployment, live validation, security approval, team merge, data migration, or operational authority.

Escalate rather than continue when the user asks for design discovery, architecture choice, unknown-cause debugging, incident response, research, operations, durable knowledge promotion, or current-state investigation. Those routes have different artifact and proof contracts.

Block if source truth, parent Slice identity, authority, or previous gate state is missing enough that any `slice.md` intent block would be invented. Do not fill gaps from memory, chat implication, or optimism; ask the narrow question or route to the safer variant. Never use this step to authorize source mutation, branch mutation, deployment, live commands, maintenance repair, package execution, durable-domain writes, or result completion claims.
