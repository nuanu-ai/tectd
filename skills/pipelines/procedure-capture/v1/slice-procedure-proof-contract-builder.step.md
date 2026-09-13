---
id: "slice-procedure-proof-contract-builder"
kind: "internal_step"
owner_family: "slice_variant"
pipeline_id: "slice.custom-procedure-capture"
step_id: "slice-procedure-proof-contract-builder"
entry_gate: false
public_trigger: false
native_skill_discovery: false
body_path: "capabilities/instructions/pipelines/slice-variants/procedure-capture/slice-procedure-proof-contract-builder.step.md"
source_manifest: "capabilities/pipelines/slice-variants/procedure-capture.pipeline.json"
legacy_skill_ref: "slice-procedure-proof-contract-builder"
architecture_refs:
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#hybrid-implementation-operation-and-procedure-capture-variants"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19"
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#procedure-and-research-slice-variants"
---

# Procedure Proof Contract Builder

## Overview
This skill defines the proof contract for a captured procedure candidate. Its core rule is to turn a useful but one-off workflow into auditable proof requirements before anyone can claim it is repeatable, safe, reusable, or ready for durable promotion.

It is a declaration skill only: it does not execute the procedure, collect missing evidence, validate the result, accept durable promotion, mutate a runbook, or activate a skill.

## When to Use
Use this inside `slice.custom-procedure-capture` after the source event, captured steps, normalized procedure, existing-match outcome, durable target candidate, and authority-risk notes are available or explicitly marked missing. Select it when the next decision depends on proof requirements for evidence classes, repeatability proof, authority proof, safety/rollback proof, freshness, negative proof, terminal readiness, or blocked proof states.

Do not use it to run checks, validate a procedure now, scrub secrets, score reuse fit, write the final proposal, approve promotion, mutate a runbook, create an active skill, or collect live/local evidence. Route those cases to the later manifest steps for secret safety, reuse fit, validation, proposal writing, skill-candidate routing, promotion gate, result writing, or operational execution.

## Source Contract
Ground this behavior in `capabilities/pipelines/slice-variants/procedure-capture.pipeline.json` step `slice-procedure-proof-contract-builder`, which produces `proof-contract.md`, gates `proof_contract_declared`, and can only advance to `ready_for_next_step` or `stop_or_handoff`.

Architecture anchors are `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#hybrid-implementation-operation-and-procedure-capture-variants`, `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#procedure-and-research-slice-variants`, `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6`, `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19`, and `docs/architecture/master-plugin-target-architecture-capability-atom-shard-map.html#pipeline.slice.procedure_capture.proof.contract.builder`. Manifest and atom anchors include `pipeline.slice.procedure-capture`, `capabilities/pipelines/slice-variants/procedure-capture.pipeline.json#step_graph.steps.slice-procedure-proof-contract-builder`, and `capabilities/pipelines/slice-variants/procedure-capture.pipeline.json#step_graph.steps.slice-procedure-proof-contract-builder.invokes.slice-procedure-proof-contract-builder`.

## Operating Procedure
1. Confirm this is a procedure-capture proof contract, not proof execution. Require `source-event.md`, `captured-steps.md`, `normalized-procedure.md`, `existing-match-check.md`, a candidate durable target from `procedure-proposal.md` or equivalent target note, and `authority-risk.md`. If any source is missing, name the missing source and keep the terminal state blocked.
2. State the candidate claims that need proof. Separate claims such as "repeatable", "safe to run", "locally proven", "live proven", "owner approved", "promotable", "blocked", and "not reusable yet" so each claim has its own evidence row.
3. Define evidence classes. At minimum classify source artifacts, command history, logs, screenshots or UI observations, current-state snapshots, configuration or version facts, owner decisions, authority records, duplicate checks, secret-safety checks, rollback material, and negative findings.
4. Define repeatability proof without running anything. For each normalized procedure phase, list the future action or command, allowed actor, required cwd or target context, input preconditions, expected output, assertion, artifact path, and evidence that would show the step can be repeated.
5. Define authority proof. Tie each future read, write, execute, deploy, promote, or owner-judgment requirement to the authority-risk record, required approval, allowed location, and block condition when authority is absent or narrower than the proposed procedure.
6. Define safety and rollback proof. Name stop conditions, unsafe probes, secret-bearing evidence, destructive actions, rollback prerequisites, recovery evidence, and the proof needed before a later runner may attempt any risky phase.
7. Define freshness proof. For every evidence class, record source class, environment or version binding, freshness window, expiry signal, owner or reviewer for manual confirmation, and when stale evidence becomes a blocked state instead of lower confidence.
8. Define negative proof. Record how to prove a duplicate or existing procedure should be updated, the source event is insufficient, a failure path is unproven, the procedure is too context-bound, proof commands are unsafe, evidence is missing, or local evidence cannot support a live claim.
9. Define terminal readiness. `ready_for_next_step` is allowed only when every reusable or promotable claim has a matching evidence class, repeatability proof, authority proof, safety/rollback proof, freshness rule, negative proof, owner rule, local/live boundary, and missing-proof blocked state.
10. Write `proof-contract.md` as a declaration artifact. If the contract cannot satisfy the readiness rule, return `stop_or_handoff` with the exact missing proof class, blocked claim, and next routing target.

## Outputs
The only owned artifact is `proof-contract.md` for the active procedure-capture Slice. It should contain a proof matrix with these columns: candidate claim, evidence class, required source input, repeatability proof, authority proof, safety or rollback proof, freshness window, negative proof, owner confirmation, local proof boundary, live proof boundary, missing-proof blocked state, forbidden claim while proof is absent, and downstream step that may collect or audit the evidence later.

The output is a contract, not evidence. It must preserve proposal-only truth and must not include proof command output, new live checks, durable runbook edits, skill activation, promotion approval, or a result claim that the procedure works.

Terminal output is one of two states: `ready_for_next_step` with the proof matrix complete enough for later secret-safety, reuse-fit, validation, proposal, and promotion steps; or `stop_or_handoff` with named missing inputs, missing proof classes, unsafe authority gaps, stale evidence, duplicate-procedure concerns, or owner actions.

## Verification
Verify the contract by reading `proof-contract.md` against the procedure-capture manifest gate `proof_contract_declared`. Every future claim such as reusable, safe to run, repeatable, promotable, locally proven, live proven, owner-confirmed, or blocked must have named evidence and a forbidden claim while evidence is absent.

Check that all required proof classes are present: input evidence, repeatability proof, authority proof, safety/rollback proof, freshness, negative proof, and terminal readiness. Check that local proof and live proof are separated, owner confirmation is required for manual or authority-sensitive evidence, failure proof is not skipped, and blocked proof states are visible.

Also check the negative boundary: no evidence was collected, no commands were run, no procedure was executed, no durable domain was mutated, no active skill was created, no runbook was updated, and no promotion was approved. A valid contract makes later proof auditable; it never substitutes for later proof.

## Failure Modes
Use `stop_or_handoff` when required source material, the captured step list, the normal procedure form, authority-risk record, existing-match record, target candidate, owner, freshness rule, local/live boundary, failure proof, or blocked state is missing. Block instead of inventing proof when the procedure depends on private credentials, unsafe live probes, unavailable environments, manual owner judgment, stale logs, or a duplicate runbook that should be updated first. The named input may be `normalized-procedure.md`, but lack of that artifact is a blocked source condition, not permission to infer the missing proof contract.

Route away when the user wants to execute the procedure, gather evidence, validate output, scrub secrets, decide reuse fit, write a runbook draft, approve promotion, or author a skill. This skill can make future proof auditable; it cannot make the procedure reusable or promotable by itself.

Forbidden actions: do not execute captured commands, run live probes, create `evidence/` or `logs/` contents, update durable KB/runbook/source files, accept promotion, activate a skill, broaden authority, hide stale evidence, collapse local proof into live proof, or mark a blocked proof class as waived without an explicit later authority gate.
