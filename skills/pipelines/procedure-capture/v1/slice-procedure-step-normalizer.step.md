---
id: "slice-procedure-step-normalizer"
kind: "internal_step"
owner_family: "slice_variant"
pipeline_id: "slice.custom-procedure-capture"
step_id: "slice-procedure-step-normalizer"
entry_gate: false
public_trigger: false
native_skill_discovery: false
body_path: "capabilities/instructions/pipelines/slice-variants/procedure-capture/slice-procedure-step-normalizer.step.md"
source_manifest: "capabilities/pipelines/slice-variants/procedure-capture.pipeline.json"
legacy_skill_ref: "slice-procedure-step-normalizer"
architecture_refs:
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#hybrid-implementation-operation-and-procedure-capture-variants"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19"
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#procedure-and-research-slice-variants"
---

# Slice Procedure Step Normalizer

## Overview

This skill normalizes extracted procedure events into candidate repeatable steps. It preserves source truth while making the workflow legible: ordered step, actor, inputs, cwd/context, preconditions, action, expected output, proof signal, stop condition, unknowns, parameter placeholders, and source support.

Classification: `skill_body`. No external skill body is adapted by this step. This is a proposal-only procedure-capture step; it does not execute the procedure, write a canonical runbook, create a skill, mutate durable libraries, or claim acceptance, promotion, or future success.

## When to Use

Use this inside `slice.custom-procedure-capture` after `slice-procedure-event-extractor` has produced a concrete `source-event.md` trace and before `slice-procedure-generalization-shaper` separates durable procedure shape from one-off details. Select it when the source event exists but still mixes shell commands, tool calls, file edits, decisions, approvals, observations, retries, proof snippets, local paths, and unresolved gaps that must become `captured-steps.md`.

Do not use it before procedure-capture entry gating, source context loading, or event extraction. Do not use it after `captured-steps.md` is already sufficient and the next need is generalization, existing-runbook matching, authority/risk classification, proof-contract design, secret scrubbing, reuse-fit evaluation, durable target selection, proposal writing, promotion approval, result writing, or skill-candidate routing.

Non-triggers include requests to run the captured commands, follow an existing runbook, change a shared runbook, author a skill body, accept a promotion, mutate durable knowledge, deploy, inspect live systems, or claim that the captured workflow works in the future.

## Source Contract

Ground this step in `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#hybrid-implementation-operation-and-procedure-capture-variants`, `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#procedure-and-research-slice-variants`, `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6`, `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19`, and `docs/architecture/master-plugin-target-architecture-capability-atom-shard-map.html#pipeline.slice.procedure_capture.step.normalizer`.

The owning manifest is `capabilities/pipelines/slice-variants/procedure-capture.pipeline.json`, step `slice-procedure-step-normalizer`. The manifest marks this as a required skill step that invokes `skill:slice-procedure-step-normalizer`, produces `captured-steps.md`, gates on `captured_steps_normalized`, fails by `stop_or_handoff`, and reaches `ready_for_next_step` only after captured steps are ordered and source-backed.

Required source inputs are `source-event.md`, source context or explicit context gaps, session commands/actions, artifacts/log references, observed decisions, proof references, authority posture, and secret-risk notes. Useful optional inputs are `README.md`, `source-slice-link.md`, command output snippets, file/artifact paths, user approval evidence, failure/retry observations, and handoff notes from the source-context loader. External references: none for this step.

## Operating Procedure

1. Confirm prerequisites. Require a concrete `source-event.md` with trigger, actors, chronological actions, observations, decisions, and proof references. If the trace is missing, vague, unsourced, or only describes desired future behavior, return `stop_or_handoff` to event extraction or source-context loading.
2. Build the evidence inventory. List source-event anchors, commands/tool calls, file edits, artifact/log references, user approvals, observed outputs, failed variants, final proof snippets, secret-risk notes, and context gaps. Treat every later step as unsupported until it maps to this inventory.
3. Extract candidate actions without changing meaning. Include commands, tool calls, file edits, checks, approvals, environment operations, branch decisions, observations that caused branching, recovery actions, and final proof collection. Preserve failed attempts when they reveal a future stop condition, alternate path, or recovery step.
4. Order by causal sequence. Remove transcript noise, but do not erase meaningful retries. Merge duplicate attempts only when the purpose, input, and outcome match; otherwise represent them as branch notes, recovery notes, or blocked variants.
5. Normalize each ordered step into a stable shape: step number, action verb, actor, required inputs, cwd/context, preconditions, exact command/action when safe, parameter placeholders, expected output, proof signal, stop condition, source reference, and dependency on prior steps.
6. Separate invariant from parameter. Mark project names, repo paths, branch names, hosts, ports, credentials, account IDs, object IDs, dates, local filenames, and user-specific surfaces as parameter placeholders unless the source proves they are invariant. Replace raw secrets with redaction markers and a handoff note for `slice-procedure-secret-safety-scrubber`.
7. Attach proof and stop gates. Each step needs a proof signal or an explicit missing-proof blocker. Stop conditions must cover missing authority, destructive risk, secret exposure, stale source, failed proof, unsafe live-system state, unresolved decision, unknown actor, missing cwd/context, or dependency on unavailable artifacts.
8. Preserve downstream boundaries. Do not decide existing-match status, generalize the procedure, classify authority/risk, design the proof contract, scrub secrets beyond local redaction markers, select durable target, write a proposal, approve promotion, write final result truth, or route to active skill creation. Leave structured notes for the owning downstream step.
9. Write the handoff block. End with parameters, unknowns, missing evidence, unsafe material to scrub, assumptions that must not become rules, downstream owner routes, and whether the artifact can advance to `slice-procedure-generalization-shaper`.

## Outputs

The required output is `captured-steps.md`. Use a table or numbered list with these fields for each step: `step_id`, actor, trigger or dependency, inputs, cwd/context, preconditions, action, parameter placeholders, expected output, proof signal, stop condition, source reference, unknowns, and next-step dependency.

Also include a summary block with source inputs inspected, evidence coverage, merged retries, preserved failed variants, parameter list, secret-safety flags, missing evidence, downstream owner notes, gate `captured_steps_normalized`, and terminal state.

Use `ready_for_next_step` only when the captured steps are ordered, source-backed, specific enough for generalization, and every step has either a proof signal or an explicit blocker. Use `stop_or_handoff` with a concrete reason such as `blocked_missing_source_event`, `blocked_unsourced_step`, `blocked_secret_risk`, `blocked_unknown_actor`, `blocked_missing_context`, `blocked_missing_proof`, `blocked_unsafe_stop_condition`, or `blocked_wrong_owner`.

The output is a candidate procedure capture artifact. It is not `normalized-procedure.md`, not an existing-match verdict, not an authority-risk verdict, not a proof contract, not a secret-safety proof, not `procedure-proposal.md`, not a durable runbook, not a KB update, not a promotion decision, not a final result, and not a skill body.

## Verification

Run `node tools/validate-internal-skill-body-quality.mjs --skill slice-procedure-step-normalizer` and `node tools/validate-internal-skill-trigger-fixtures.mjs --skill slice-procedure-step-normalizer`. Also parse the two owned fixture JSON files, scan this skill for exactly the seven Layer 6B H2 sections, run a scoped whitespace/final-newline scan, and run `git diff --check --` on this skill plus its two fixture files.

For content verification, inspect `captured-steps.md`: every captured step must have source support, actor, inputs, cwd/context or context marker, preconditions, action, parameter placeholders, expected output, proof signal or blocker, stop condition, dependency notes, and unknowns. Unknowns must stay visible instead of becoming confident procedure text.

Check that the sequence can feed `slice-procedure-generalization-shaper`: one-off details are marked as parameters, invariants are justified by source evidence, unsafe values are redacted or flagged, useful failed variants are preserved, and missing proof routes to `stop_or_handoff`. Confirm the artifact contains no execution, canonical-runbook changes, durable-library changes, promotion claim, active skill creation, or final acceptance claim.

## Failure Modes

Return `stop_or_handoff` when `source-event.md` is missing, the event trace is too vague, steps cannot be ordered from evidence, required actors or cwd/context are unknown, command output or proof is absent, the workflow depends on raw secrets, a step would require live execution to understand, or normalizing would create an unsupported reusable rule.

Escalate or hand off by owner: source-context gaps go to `slice-procedure-source-context-loader`; missing or weak event facts go to `slice-procedure-event-extractor`; parameter/invariant shaping goes to `slice-procedure-generalization-shaper`; duplicate or existing-runbook questions go to `slice-procedure-existing-match-checker`; authority/risk gaps go to `slice-procedure-authority-and-risk-classifier`; proof-contract gaps go to `slice-procedure-proof-contract-builder`; secret exposure goes to `slice-procedure-secret-safety-scrubber`; reuse or target questions go to reuse-fit or durable-target steps; proposal, promotion, result, and skill-candidate requests go to their named downstream owners.

Route away when the user wants to execute commands, deploy, mutate files outside the candidate artifact, update canonical runbooks, write durable KB, create or activate skills, approve promotion, perform maintenance execution, or claim the captured procedure has been accepted. Preserve a zero-progress note so the procedure-capture Slice records what blocked normalization and which owner must act next.
