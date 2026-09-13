---
id: "slice-procedure-capture-entry-gate"
kind: "internal_step"
owner_family: "slice_variant"
pipeline_id: "slice.custom-procedure-capture"
step_id: "slice-procedure-capture-entry-gate"
entry_gate: true
public_trigger: false
native_skill_discovery: false
body_path: "capabilities/instructions/pipelines/slice-variants/procedure-capture/slice-procedure-capture-entry-gate.step.md"
source_manifest: "capabilities/pipelines/slice-variants/procedure-capture.pipeline.json"
legacy_skill_ref: "slice-procedure-capture-entry-gate"
architecture_refs:
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#hybrid-implementation-operation-and-procedure-capture-variants"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19"
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#procedure-and-research-slice-variants"
---

# Procedure Capture Entry Gate

## Overview

This is the Tect-owned entry gate for `slice.custom-procedure-capture`. It decides whether an unexpected useful user-agent workflow is a durable process candidate and, if so, opens only a proposal path; it does not write the final procedure, execute commands, promote durable knowledge, or turn a one-off action into a hard rule.

Classification: `reference_adapter_skill`. `superpowers:writing-plans` is used only for adapted discipline: name the candidate task boundary, the expected implementation plan shape for later work, and the verification need. The external skill is not the canonical implementation of this gate.

## When to Use

Use when all are true:

- A source Slice or session event produced an unexpected useful user-agent workflow.
- The event has source/proof material to preserve, such as captured commands or steps, logs, artifacts, decisions, checks, or final proof.
- The workflow solved or prevented a real problem and has recurrence likelihood, forgetting risk, or a clear future handoff value.
- The right next action is to declare a proposal-only durable process candidate: procedure note, runbook candidate, command recipe, proof-order template, workflow rule, or later skill candidate.

Do not use when:

- An existing runbook should be followed, updated through its owner, or executed now.
- The work is ordinary development, lightweight TDD, debug/root-cause, operational execution, hybrid implementation, research-to-durable-KB, setup, migration, import, or capability authoring.
- The user is asking for an immediate durable write, package update, deployment, live-system action, or skill creation/activation.
- The event is a one-off convenience with no recurrence, proof, handoff value, or forgetting risk.
- There is no source event to preserve.

## Source Contract

Read the active Runtime packet or handoff, the source event summary, captured commands or steps, evidence references, desired durable target candidate, current authority state, and secret-risk posture. If any required source/proof input is absent, this gate must block or hand off instead of inventing a procedure candidate.

Grounding:

- `capabilities/pipelines/slice-variants/procedure-capture.pipeline.json`, step `slice-procedure-capture-entry-gate`, gate `procedure_candidate_declared`, output `slice.md`, failure route `stop_or_handoff`, terminal state `ready_for_next_step`.
- `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6` for the Procedure capture Slice completion boundary and proof requirement.
- `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19` for routing unexpected custom operations through procedure/runbook gates without automatic promotion.
- `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#hybrid-implementation-operation-and-procedure-capture-variants` for Procedure Required/Optional artifacts, forbidden claims, and terminal truth.
- `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#procedure-and-research-slice-variants` for the boundary between procedure capture and research-to-durable-KB.
- Reference source: `skills/references/superpowers/writing-plans/SKILL.md`.

## Operating Procedure

1. Confirm the source event. Identify the source Slice, session, handoff, or result that produced the workflow, and list the evidence references available now.
2. Test candidate fit. Require at least one durable process signal: recurrence likelihood, forgetting risk, repeated manual judgment, reusable proof order, command sequence, safety checklist, or future operator handoff value.
3. Reject alternate routes early. If the real work is existing runbook execution, research, adoption/import, normal implementation/debug/ops, package work, live action, or capability authoring, return the matching routed state instead of stretching procedure capture.
4. Screen sensitive material. Classify whether the source contains secrets, raw credentials, tokens, private hostnames, unsafe environment details, or unredacted private material. If safe summary/redaction is not possible at entry, block with `blocked_secret_risk`.
5. Declare the proposal boundary. Name the candidate type, source basis, proposed durable target lane if known, authority posture, secret-risk posture, rejected alternate routes, and why the gate is proposal-only.
6. Adapt planning discipline narrowly. Capture candidate task boundaries, expected verification needs, future stop conditions, and whether a later proposal would need a command recipe, proof-order template, runbook draft, procedure note, workflow rule, or skill-candidate route. Do not draft the full implementation plan here.
7. Write only `slice.md`. It must record `procedure_candidate_declared`, selected variant `slice.custom-procedure-capture`, source event reference, evidence refs, candidate type, recurrence/forgetting signal, authority and secret-risk flags, durable target routing hypothesis, terminal state, and next step `slice-procedure-source-context-loader`.
8. Preserve forbidden-action boundaries. Do not run commands, mutate repos or worktrees, write canonical procedures/runbooks/KB pages, activate skills, install packages, deploy, start operations, or promote durable knowledge from this entry gate.
9. Stop when proof, authority, or safety is missing. Use `stop_or_handoff` with the exact missing condition instead of creating an attractive but unauditable procedure candidate.

## Outputs

Primary output: `slice.md`.

`slice.md` must include: selected variant, source event reference, `procedure_candidate_declared`, candidate type, recurrence likelihood or forgetting risk, candidate task boundaries, verification needs, known evidence refs, authority posture, secret-risk posture, rejected alternate routes, durable target routing hypothesis, next step, and terminal state.

Success terminal state: `ready_for_next_step`, with next step `slice-procedure-source-context-loader`.

Blocked or routed states: `stop_or_handoff`, `requires_existing_runbook_execution`, `requires_research_variant`, `requires_adoption_pipeline`, `requires_operational_execution`, `requires_skill_authoring`, `blocked_missing_source_event`, `blocked_secret_risk`, or `blocked_missing_authority`.

The output may propose a later procedure/runbook/command-recipe/proof-template/skill-candidate lane, but it must state no durable runbook or skill mutation occurred.

## Verification

Verify `slice.md` before handoff:

- It proves the trigger was an unexpected useful user-agent workflow with source/proof material, not normal execution of an existing runbook or ordinary Slice work.
- It records captured commands or steps, logs, artifacts, decisions, or proof references; otherwise it blocks on missing source evidence.
- It captures task and verification boundaries adapted from `superpowers:writing-plans` without writing a full implementation plan.
- It records secret-risk posture and either safe redaction posture or `blocked_secret_risk`.
- It names only a proposal-only durable target routing hypothesis and confirms no durable procedure, runbook, KB, or skill source was changed.
- It preserves `procedure_candidate_declared`, terminal state `ready_for_next_step`, and handoff to `slice-procedure-source-context-loader`.

Fixture checks: run `node tools/validate-internal-skill-body-quality.mjs --skill slice-procedure-capture-entry-gate` and `node tools/validate-internal-skill-trigger-fixtures.mjs --skill slice-procedure-capture-entry-gate`.

## Failure Modes

Block with `blocked_missing_source_event` when there is no source Slice, session event, captured steps, commands, logs, artifacts, decisions, or proof to preserve.

Route with `requires_existing_runbook_execution` when an existing runbook already covers the process and should be followed or revised by its owner instead of duplicated.

Route with `requires_research_variant` when the main need is evidence gathering, claim reconciliation, source corpus synthesis, or durable KB promotion.

Route with `requires_adoption_pipeline` when the request is import, migration, setup, packaging, activation, or compatibility work for skills/runbooks/procedures.

Route with `requires_operational_execution` when the user is asking for an approved live operation, deployment, rollback, or command execution rather than capture of a completed source event.

Route with `requires_skill_authoring` when the user explicitly wants a new or updated active skill. Procedure capture can later feed that route, but this entry gate cannot perform it.

Block with `blocked_secret_risk` when the captured process contains secrets, raw credentials, tokens, unsafe environment details, or unredacted private material that cannot be safely summarized at the entry gate.

Block with `blocked_missing_authority` when the candidate requires durable mutation, package changes, deployment, operation execution, or promotion authority that this step does not have.
