---
id: "slice-lightweight-contract-writer"
kind: "internal_step"
owner_family: "slice_variant"
pipeline_id: "slice.lightweight-tdd-development"
step_id: "slice-lightweight-contract-writer"
entry_gate: false
public_trigger: false
native_skill_discovery: false
body_path: "capabilities/instructions/pipelines/slice-variants/lightweight-tdd/slice-lightweight-contract-writer.step.md"
source_manifest: "capabilities/pipelines/slice-variants/lightweight-tdd.pipeline.json"
legacy_skill_ref: "slice-lightweight-contract-writer"
architecture_refs:
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#lightweight-development"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19"
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#lightweight-and-debug-slice-variants"
---

# Lightweight Contract Writer

## Overview

Create or refresh the `slice.md` contract for a selected Lightweight TDD Slice. The core rule is: lightweight means fewer artifacts, not weaker proof. This skill locks the single lightweight lifecycle, authority boundary, artifact shape, proof contract, and escalation routes before test-target selection or implementation work starts.

## When to Use

Use after `slice.lightweight-tdd-development` has been selected, intent and acceptance checks are clear, immediate context is loaded, and lightweight workspace preflight has not exposed a blocking safety issue. The next needed artifact must be `slice.md`, and the next pipeline gate must be `authority_and_proof_contract_declared`.

Use for small understood code, config, or business-rule changes with bounded affected surface and focused local proof. Do not use for unknown root cause, architecture ambiguity, cross-component redesign, deployment or live validation dependency, missing test target, repeated failure, operational execution, research-to-KB work, result writing, promotion routing, or full design-to-execution work.

## Source Contract

Grounding sources:

- `capabilities/pipelines/slice-variants/lightweight-tdd.pipeline.json` step `slice-lightweight-contract-writer`
- `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#lightweight-development`
- `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#lightweight-and-debug-slice-variants`
- `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6`
- `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19`
- `docs/architecture/master-plugin-target-architecture-capability-atom-shard-map.html#pipeline.slice.lightweight_tdd.contract.writer`

The manifest step is required, invokes `slice-lightweight-contract-writer`, produces `slice.md`, gates on `authority_and_proof_contract_declared`, reaches `contract_ready`, and fails through `ask_user_or_escalate_full`. The relevant atom is `pipeline.slice.lightweight-tdd`, with step anchor `capabilities/pipelines/slice-variants/lightweight-tdd.pipeline.json#step_graph.steps.slice-lightweight-contract-writer`.

Required source inputs are the selected Slice or Slice candidate, parent Scope constraints, user request, expected behavior, acceptance checks, non-goals, immediate files/docs/tests already loaded by context, workspace preflight summary, authority state, branch/worktree posture, and any known proof target candidates. If any required input is absent or untrusted, record the gap instead of inventing it.

## Operating Procedure

1. Confirm the selected variant is `slice.lightweight-tdd-development`, lifecycle depth is `lightweight`, and the work still fits the lightweight trigger: small understood change, clear acceptance, bounded surface, focused proof sufficient. If the request is actually debug, full design, hybrid deploy/live, ops, research, result, or promotion work, do not write a successful contract.
2. Check the prerequisite inputs from prior lightweight steps: intent, expected behavior, non-goals, acceptance checks, immediate context, parent Scope constraints, and workspace preflight. If any are missing, write only a blocked or escalation contract with `ask_user_or_escalate_full`.
3. State the Slice identity: parent Program/Epoch/Scope when known, selected variant, lifecycle depth `lightweight`, target files or systems, owner, current lifecycle state, and whether this is new work, continuation, or repair.
4. Restate the change contract in plain terms: user request, expected behavior, non-goals, affected surface, assumptions, explicit unknowns, and the smallest useful acceptance checks. Keep unclear requirements visible instead of converting them into certainty.
5. Declare authority before work can continue: allowed read/write scope, source mutation boundary, branch or worktree posture, protected surfaces, approval gaps, and whether deploy, live, data, security, or team authority is absent. This contract may permit later source mutation only after authority is declared; it does not itself authorize implementation, test execution, deployment, live-system commands, durable-domain writes, result writing, or promotion.
6. Declare the artifact contract. Required lightweight artifacts are `README.md`, `slice.md`, `workspace-preflight.md`, `test-target.md`, `tdd-notes.md`, `implementation-notes.md`, `verification.md`, and `result.md`; `test-plan.md` may record a blocked target gap only when no ready test target exists and the contract records the missing-proof blocker or escalation route. Optional artifacts are `deployment-validation.md`, `promotion.md`, `deferred.md`, `handoff.md`, `evidence/`, and `review.md`.
7. Declare forbidden artifacts and hidden lifecycle blockers unless a typed escalation record changes variant: `design-spec.md`, `decisions/`, `cross-cutting-review.md`, `implementation-ready-spec.md`, `execution-runs/run-N/`, any second implementation plan, and heavyweight full-development artifacts.
8. Set the proof contract before test selection starts: expected RED proof or acceptance proof target, focused verification command classes, affected checks, missing-proof blocker rule, and the rule that local proof is not deployment or live proof. The next step may select the exact target, but `slice.md` must say what kind of proof is acceptable and what proof gap blocks completion.
9. Set escalation and handoff routes exactly enough for the next step to enforce them: architecture ambiguity or cross-component uncertainty routes to `slice.full-design-to-execution`; unknown root cause or repeated failure routes to `slice.debug-root-cause`; deployment or live validation dependency routes to `slice.hybrid-implementation-operation`; missing test target routes to full design-to-execution or a blocked `test-plan.md`; missing authority routes to user approval or `handoff.md`.
10. Write `slice.md` as the authoritative contract for the selected lightweight Slice. End with `contract_ready` only when variant identity, lifecycle state, authority, proof requirements, artifact set, forbidden claims, and escalation triggers are explicit. Otherwise end with `ask_user_or_escalate_full` and list the missing contract fields.

## Outputs

Primary output is `slice.md` in the selected Slice folder. It must use this shape or a local equivalent with the same fields:

1. `# <Slice name>`
2. `## Slice Identity`: parent links, selected variant `slice.lightweight-tdd-development`, lifecycle depth `lightweight`, owner, target surface, lifecycle state.
3. `## Intent And Acceptance Checks`: user request, expected behavior, non-goals, assumptions, explicit unknowns, acceptance checks.
4. `## Source Inputs`: parent Scope constraints, loaded files/docs/tests, workspace preflight, authority state, branch/worktree posture.
5. `## Authority Boundary`: allowed reads/writes, source mutation boundary for later steps, protected surfaces, missing approvals.
6. `## Artifact Contract`: required, optional, forbidden-unless-escalated, and hidden second lifecycle rule.
7. `## Proof Contract`: RED or acceptance proof target type, focused verification classes, affected checks, missing-proof blocker, local-proof-not-live-proof rule.
8. `## Escalation And Handoff Routes`: full, debug, hybrid, user approval, or handoff conditions.
9. `## Forbidden Claims And Actions`: no implementation/test/deploy/result/promotion claims from this contract step.
10. `## Terminal State`: `contract_ready` or `ask_user_or_escalate_full`.

The file may reference downstream artifacts that later steps will write, but this skill does not create `workspace-preflight.md`, `test-target.md`, `tdd-notes.md`, `test-plan.md`, `implementation-notes.md`, `verification.md`, `result.md`, deployment validation, promotion, deferred, handoff, evidence, or review files. The only successful terminal state is `contract_ready`; otherwise the output is a blocked or escalation note inside `slice.md` with terminal state `ask_user_or_escalate_full`.

## Verification

Verify the body with `node tools/validate-internal-skill-body-quality.mjs --skill slice-lightweight-contract-writer` and trigger coverage with `node tools/validate-internal-skill-trigger-fixtures.mjs --skill slice-lightweight-contract-writer`.

Content verification checks that `slice.md` answers: why lightweight was selected, what source inputs were used, what is in scope, what proof is required, what authority exists, which artifacts are required, which artifacts are forbidden, what cannot be claimed, and when the Slice must escalate. The gate `authority_and_proof_contract_declared` is satisfied only when the authority boundary and proof contract are explicit. The contract must not claim implementation, test execution, verification, result, promotion, deployment, live behavior, or workspace mutation has happened.

## Failure Modes

Use `ask_user_or_escalate_full` when intent, acceptance checks, parent Scope, immediate context, workspace safety, authority, or proof expectations are not explicit enough to contract. Do not invent missing authority or silently downgrade proof requirements.

Escalate instead of writing a lightweight contract when the work is ambiguous, architectural, cross-component, live/deploy dependent, security-sensitive, team-merge dependent, missing a plausible test or acceptance proof target, or already showing repeated failure. Route unknown cause to debug; route deploy/live dependency to hybrid implementation plus operation; route broad design work to full design-to-execution.

Block rather than continue if the requested contract would hide a second lifecycle, treat local checks as live proof, skip RED or acceptance proof, omit `result.md`, authorize protected branch mutation, mutate a workspace before authority is declared, claim deployment/live proof, claim a result or promotion, or create heavyweight artifacts under a lightweight Slice without a typed escalation record.
