---
id: "slice-debug-contract-writer"
kind: "internal_step"
owner_family: "slice_variant"
pipeline_id: "slice.debug-root-cause"
step_id: "slice-debug-contract-writer"
entry_gate: false
public_trigger: false
native_skill_discovery: false
body_path: "capabilities/instructions/pipelines/slice-variants/debug-root-cause/slice-debug-contract-writer.step.md"
source_manifest: "capabilities/pipelines/slice-variants/debug-root-cause.pipeline.json"
legacy_skill_ref: "slice-debug-contract-writer"
architecture_refs:
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#debug-root-cause-variant"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19"
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#lightweight-and-debug-slice-variants"
---

# Slice Debug Contract Writer

## Overview

Create the `slice.md` contract for `slice.debug-root-cause`. The contract freezes the debug scope, source inputs, proof order, authority boundary, stop conditions, and handoff route before the investigation can drift into fixes.

This is a reference adapter for `superpowers:writing-plans`: adapt its exact task and verification discipline into a Slice-local debug contract. Do not create an implementation plan, execute a task, mutate source, deploy, or call the bug fixed from this skill.

## When to Use

Use when Runtime has selected the debug/root-cause variant for a bug, regression, failing proof, or behavior mismatch where the root cause is unknown and the next safe move is to define the Slice contract. Minimum inputs are: parent Scope or Slice candidate, expected behavior, observed behavior, affected surface, authority state, available logs/tests/diffs/runtime state, and any reproduction or unable-to-reproduce status already known.

Do not use for a vague bug report with no expected-versus-observed delta, a bounded fix whose cause is already understood, live incident response, rollback/deploy work, pure research, durable KB grooming, or full design/spec work. Route those to symptom capture, lightweight TDD, operational execution, research, or full design-to-execution.

## Source Contract

- Architecture anchors: `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#debug-root-cause-variant`, `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#lightweight-and-debug-slice-variants`, `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6`, and `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19`.
- Manifest anchor: `capabilities/pipelines/slice-variants/debug-root-cause.pipeline.json#step_graph.steps.slice-debug-contract-writer`.
- Manifest gates: `scope_boundary_declared` and `proof_order_required`.
- Manifest output: `slice.md`; terminal state: `contract_ready`; failure route: `block_missing_contract`.
- Required downstream artifacts that this contract must reference but not write: `symptom.md`, `reproduction.md`, `evidence-log.md`, `hypotheses.md`, `root-cause.md`, `fix-plan.md` or `no-fix-result.md`, `verification.md`, and `result.md`.
- External reference: `skills/references/superpowers/writing-plans/SKILL.md`, used only for concrete task granularity, artifact shape, and verification expectations.

## Operating Procedure

1. Confirm selection and minimum inputs. Verify the selected variant is `slice.debug-root-cause`, root cause is unknown or unproven, and the entry/context inputs include expected behavior, observed behavior, target surface, authority state, and available evidence sources. If the delta is missing, block and route to symptom capture.
2. Declare the debug scope boundary. Name one primary broken behavior, affected component or workflow, environment, time window, version/branch/runtime context, parent object, explicit non-goals, and related symptoms that are out of scope. Split multiple independent failures into follow-up Slice candidates.
3. Materialize `slice.md` as a contract, not a plan. Use sections: `Identity`, `Debug Scope`, `Symptom Inputs`, `Reproduction Contract`, `Evidence Contract`, `Hypothesis Discipline`, `Proof Order`, `Authority Boundary`, `Escalation And Stop Conditions`, `Next Step`, and `Handoff`.
4. Adapt writing-plans discipline into diagnostic task rows. Each row must have a task name, source input, read or inspection action, proof target, expected observation or decision rule, output artifact, and blocker route. Do not write code snippets or implementation steps unless a later fix strategy has root-cause proof.
5. Define the reproduction contract. Record the current reproduction state, the smallest reproducible proof target, acceptable unable-to-reproduce criteria, and which downstream artifact must hold the result. Completion later requires reproduction or an explicit unable-to-reproduce record.
6. Define the evidence and hypothesis boundaries. Separate observed facts from guesses, list evidence already checked with freshness, list evidence still required, require `hypotheses.md` for candidates and rejections, and state that stacked guesses are blocked.
7. Set proof order before any fix. The contract must require symptom confirmation, reproduction or unable-to-reproduce, evidence order, hypothesis ledger, root-cause decision, regression proof, local verification, and only then fix strategy. If live validation is needed, record it as a handoff requirement, not as authority granted here.
8. Declare authority and forbidden actions. This skill can write the contract only. It must not grant source mutation, package execution, deployment, branch mutation, durable-domain writes, live-system commands, pipeline execution, or completion claims.
9. Route terminal state. Mark manifest terminal state `contract_ready` only when scope and proof order are explicit. Otherwise route through manifest failure `block_missing_contract` to symptom capture, context loading, reproduction building, evidence-order planning, root-cause decision, operational execution, full design-to-execution, or human authority.

## Outputs

Write exactly one primary artifact: `slice.md` in the selected Slice folder. It must contain the sections listed above, identify the owning manifest and debug variant, and name the next allowed artifact or skill. The contract may link to required downstream files, but it does not create `fix-plan.md`, `patch.md`, `verification.md`, `result.md`, `promotion.md`, or `handoff.md`.

Successful terminal state: `contract_ready`. Failure route: `block_missing_contract`. Valid handoff routes are diagnostic continuation, evidence gathering, root-cause decision, no-fix result, fix strategy after root cause, full design escalation, operational escalation, or human decision. The output must make the next worker able to continue without reading chat history.

## Verification

Before marking `contract_ready`, check `slice.md` against this checklist:

- References `slice.debug-root-cause`, `capabilities/pipelines/slice-variants/debug-root-cause.pipeline.json`, and at least one `docs/architecture/*.html` source.
- Contains one bounded debug target, expected behavior, observed behavior, affected surface, authority state, and non-goals.
- Has diagnostic task rows with task, source input, action, proof target, expected observation, output artifact, and blocker route.
- Requires reproduction or explicit unable-to-reproduce, evidence order, hypothesis ledger, root-cause decision, regression proof, local verification, and result proof.
- Names live validation as not required, unavailable, or handoff-required; it must not silently claim live proof.
- Contains no fix-before-root-cause, stacked guesses, "fix and see", implementation plan execution, mutation authority, deployment authority, live command authority, or completion claim.

## Failure Modes

Use `block_missing_contract` when the expected-versus-observed delta is missing, parent Slice/Scope is unknown, the target is broader than one debug lifecycle, proof order is absent, authority is unclear, or the next worker would have to guess.

Route to symptom capture when the report is vague. Route to reproduction builder when no reproduction target or unable-to-reproduce criteria exists. Route to evidence-order planning when logs, traces, tests, diffs, runtime state, or comparison evidence are unordered. Route to root-cause decision when evidence exists but the cause is still unproven. Route to lightweight TDD when the cause and bounded fix are already known. Route to full design-to-execution when the fix implies architecture or cross-component redesign. Route to operational execution or hybrid work when live impact, rollback, deploy authority, or production mutation dominates.

If three failed fixes already happened, stop the debug fast path and route to architecture discussion or full design-to-execution. If evidence or authority is unavailable, write the missing source, access, proof, or decision into the blocked contract and hand off; do not weaken the proof target.
