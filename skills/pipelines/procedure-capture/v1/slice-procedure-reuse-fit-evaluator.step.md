---
id: "slice-procedure-reuse-fit-evaluator"
kind: "internal_step"
owner_family: "slice_variant"
pipeline_id: "slice.custom-procedure-capture"
step_id: "slice-procedure-reuse-fit-evaluator"
entry_gate: false
public_trigger: false
native_skill_discovery: false
body_path: "capabilities/instructions/pipelines/slice-variants/procedure-capture/slice-procedure-reuse-fit-evaluator.step.md"
source_manifest: "capabilities/pipelines/slice-variants/procedure-capture.pipeline.json"
legacy_skill_ref: "slice-procedure-reuse-fit-evaluator"
architecture_refs:
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#procedure-and-research-slice-variants"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19"
---

# Procedure Reuse Fit Evaluator

## Overview
This skill decides whether a captured procedure is worth reusing beyond the source event. Core rule: a useful one-off workflow is not enough; the candidate must be safe, repeatable, evidenced, non-duplicate, and general enough before it can move toward a runbook, procedure note, command recipe, proof template, or skill candidate.

Classification: `skill_body`. No external skill body is adapted by this step.

## When to Use
Use this inside `slice.custom-procedure-capture` after the captured workflow has source evidence, normalized steps, existing-match status, a candidate-only durable target in `procedure-proposal.md`, authority/risk labels, proof requirements, and secret-safety review available or explicitly blocked. Typical triggers are "should this become a reusable procedure?", "is this only local context?", "is this a skill candidate?", or "should we update an existing artifact instead?"

Do not use it to start procedure capture, extract the event, normalize steps, select the durable target, scrub secrets, write a proposal, promote durable knowledge, activate a skill, follow an existing runbook, or execute an operation. If duplicate status, candidate target, authority, proof, or secret safety is unresolved, route back to the owning earlier step rather than scoring reuse fit from partial evidence.

## Source Contract
Ground this skill in `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#procedure-and-research-slice-variants`, `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6`, and `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19`.

The owning manifest is `capabilities/pipelines/slice-variants/procedure-capture.pipeline.json`, step `slice-procedure-reuse-fit-evaluator`. The manifest marks this required validator as invoking `skill:slice-procedure-reuse-fit-evaluator`, producing `reuse-fit.md`, gating on `reuse_fit_recorded`, failing by `stop_or_handoff`, and allowing only `ready_for_next_step` after a grounded verdict.

Use the manifest anchor `capabilities/pipelines/slice-variants/procedure-capture.pipeline.json#step_graph.steps.slice-procedure-reuse-fit-evaluator.invokes.slice-procedure-reuse-fit-evaluator` and atom `pipeline.slice.procedure_capture.reuse.fit.evaluator`. Procedure capture remains proposal-only: this step writes only `reuse-fit.md`; it does not write canonical storage, create active skills, write proposals, scrub secrets, promote, execute procedures, deploy, run live commands, mutate source repos, or write final result truth.

## Operating Procedure
1. Confirm prerequisite evidence. Require `source-event.md`, `captured-steps.md`, `normalized-procedure.md`, `existing-match-check.md`, candidate-only `procedure-proposal.md` from `slice-procedure-durable-target-selector`, `authority-risk.md`, `proof-contract.md`, and `secret-safety.md`, or a clear blocked note explaining why one input is absent. If the candidate lacks enough source truth to evaluate reuse, return `stop_or_handoff`.
2. Build the reuse signature. State the procedure purpose, trigger, actors, target context, candidate target, inputs, parameters, preconditions, ordered actions, proof requirements, failure handling, authority class, secret-safety status, and existing-match result in a compact comparison block. Confirm that `procedure-proposal.md` is candidate-only and that gate `target_selected_as_candidate_only` has not been treated as approval.
3. Separate reusable shape from local residue. Mark details that can become parameters apart from details tied to one workspace, one user, one account, one incident, one secret, one branch, one temporary service state, or one undocumented manual judgment.
4. Score the candidate across seven checks: recurrence likelihood, parameterized generality, evidence quality, proofability, safety and authority, duplicate or update risk, and candidate-target consistency. Use explicit evidence for each score; uncertainty lowers fit instead of being smoothed over.
5. Apply verdict criteria. Use `reusable_procedure`, `runbook_candidate`, `command_recipe_candidate`, or `proof_template_candidate` only when the process is repeatable, parameterized, proofable, safe within declared authority, non-duplicative or update-aware, and consistent with the candidate target. Use `context_bound` when value depends on one local environment, private account state, incident, branch, or undocumented judgment. Use `unsafe` when authority, destructive action, secret exposure, live-system assumptions, rollback, or proof is unresolved. Use `too_vague` when trigger, ordered steps, inputs, expected output, proof, or failure handling cannot be reconstructed. Use `skill_candidate` only when the behavior is repeatable agent behavior with stable triggers, inputs, non-triggers, safe authority boundaries, and validation pressure cases; it remains candidate-only.
6. Assign one primary verdict: `reusable_procedure`, `runbook_candidate`, `command_recipe_candidate`, `proof_template_candidate`, `skill_candidate`, `duplicate_update_only`, `context_bound`, `unsafe`, `too_vague`, or `not_worth_durable_capture`.
7. Choose the route. Candidate-worthy verdicts may continue to validation and later proposal/candidate routing without writing those downstream artifacts. `duplicate_update_only` routes to update, merge, supersession, or existing artifact repair. `context_bound`, `unsafe`, `too_vague`, and `not_worth_durable_capture` stop or hand off with the exact reason and any follow-up work needed.
8. Write `reuse-fit.md`. Include the reuse signature, prerequisite artifact list, seven-check score matrix, candidate-target consistency, local-residue notes, primary verdict, rejected verdicts, confidence, evidence links, next owner, and terminal state. Record `reuse_fit_recorded` only when the verdict is evidence-backed and the route preserves proposal-only boundaries.

## Outputs
The required output is `reuse-fit.md`. It must include the reuse signature, prerequisite artifacts inspected, seven-check score matrix, candidate-target consistency, primary verdict, confidence, blocked or rejected alternatives, route decision, residual risks, and next manifest owner.

Allowed verdicts are `reusable_procedure`, `runbook_candidate`, `command_recipe_candidate`, `proof_template_candidate`, `skill_candidate`, `duplicate_update_only`, `context_bound`, `unsafe`, `too_vague`, and `not_worth_durable_capture`. A successful continuation uses gate `reuse_fit_recorded` and terminal state `ready_for_next_step`. Unsafe, duplicate/update-only, context-bound, vague, or under-evidenced candidates use `stop_or_handoff` and name the correct route: earlier procedure-capture step, existing artifact update, domain-owner review, maintenance, research, operational preparation, or no durable target.

This skill may recommend a candidate direction, but it must not create or edit `procedure-proposal.md`, `runbook-draft.md`, `command-recipe.md`, `skill-candidate.md`, promotion records, durable knowledge edits, active skills, secret-safety records, final result truth, or live execution evidence.

## Verification
Run `node tools/validate-internal-skill-body-quality.mjs --skill slice-procedure-reuse-fit-evaluator` and `node tools/validate-internal-skill-trigger-fixtures.mjs --skill slice-procedure-reuse-fit-evaluator`. Also parse the two owned fixture JSON files, scan this skill for exactly the seven Layer 6B H2 sections, run a scoped whitespace scan, and run `git diff --check --` on this skill plus its two fixture files.

For content verification, inspect `reuse-fit.md`: it must cite the procedure-capture manifest, name `slice-procedure-reuse-fit-evaluator`, show prerequisite evidence including candidate-only `procedure-proposal.md`, score all seven checks, choose exactly one primary verdict, explain why rejected routes were rejected, and preserve proposal-only boundaries. Verify that duplicate/update-only, unsafe, context-bound, too-vague, and not-worth-durable-capture verdicts block silent proposal or promotion, while skill-candidate verdicts remain candidate-only and do not activate a skill.

## Failure Modes
Return `stop_or_handoff` when source evidence, normal procedure shape, existing-match status, candidate-only target selection, authority and risk labels, proof requirements, or secret-safety review are missing or contradictory. Also stop when a candidate depends on private credentials, unsafe live actions, unstated operator judgment, stale evidence, one-off incident context, unresolved duplicate risk, target mismatch, or proof that cannot be declared without executing the procedure.

Route away when the user wants to execute an existing runbook, mutate durable knowledge, approve promotion, author an active skill, run validation commands, deploy, migrate, clean up, or inspect live systems. Preserve the negative verdict and evidence gaps so later maintenance can retire, update, merge, or revisit the candidate without creating another unmanaged artifact.

Record a zero-progress status when the requested action belongs to another owner, so the procedure-capture Slice remains honest about what was evaluated and what was not.
