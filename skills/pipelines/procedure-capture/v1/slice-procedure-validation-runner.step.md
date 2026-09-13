---
id: "slice-procedure-validation-runner"
kind: "internal_step"
owner_family: "slice_variant"
pipeline_id: "slice.custom-procedure-capture"
step_id: "slice-procedure-validation-runner"
entry_gate: false
public_trigger: false
native_skill_discovery: false
body_path: "capabilities/instructions/pipelines/slice-variants/procedure-capture/slice-procedure-validation-runner.step.md"
source_manifest: "capabilities/pipelines/slice-variants/procedure-capture.pipeline.json"
legacy_skill_ref: "slice-procedure-validation-runner"
architecture_refs:
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#procedure-and-research-slice-variants"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19"
---

# Procedure Validation Runner

## Overview

This skill is the validation gate for `slice.custom-procedure-capture` procedure proposals. It checks whether an ad hoc workflow is complete, reproducible, safe, non-duplicate, authority-bounded, and provable enough to move toward proposal writing; it does not execute the procedure or promote it.

Core rule: validation is a claim about proposal readiness only. It must adapt the evidence-before-claim discipline from `superpowers:verification-before-completion`: name the claim, inspect fresh proof, state missing proof, and block or hand off instead of overclaiming.

## When to Use

Use this skill when all of these are true:

- The selected Slice variant is `slice.custom-procedure-capture`.
- A candidate workflow, command recipe, runbook draft, proof-order template, procedure note, or skill candidate has been extracted from source Slice/session evidence.
- The Slice is at the `slice-procedure-validation-runner` manifest step after source capture, normalization, duplicate check, authority/risk classification, proof-contract building, secret scrub, and reuse-fit review.
- Runtime needs a verdict before `procedure-proposal.md`, `promotion-gate.md`, runbook-library routing, durable-knowledge routing, or skill-candidate routing can proceed.

Do not use this skill for executing an existing runbook, running target operation commands, debugging an implementation, conducting research synthesis, writing durable KB, updating canonical runbooks, authoring active skills, deploying, installing packages, or closing a Result. Route those to the relevant operational, research, runbook-library, durable-domain, capability-authoring, or result pipeline.

## Source Contract

Ground this skill in:

- `capabilities/pipelines/slice-variants/procedure-capture.pipeline.json`, step `slice-procedure-validation-runner`
- `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#procedure-and-research-slice-variants`
- `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6`
- `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19`
- Reference adapter skill for `superpowers:verification-before-completion`

Required input artifacts are `source-event.md`, `captured-steps.md`, `normalized-procedure.md`, `existing-match-check.md`, `authority-risk.md`, `proof-contract.md`, `secret-safety.md`, `reuse-fit.md`, and any draft procedure proposal inputs such as `command-recipe.md`, `runbook-draft.md`, `skill-candidate.md`, or durable target notes. Treat missing inputs as a blocker unless the output is explicitly a handoff asking for that input.

## Operating Procedure

1. State the validation claim before checking anything: "This candidate is ready for the next procedure-capture step" or "This candidate is not ready." Do not use validation wording to imply the procedure has been executed again or that future execution will succeed.
2. Confirm the Slice context. Verify the selected variant is `slice.custom-procedure-capture`, the candidate belongs to the active Slice, and the request is proposal validation rather than target operation execution, durable mutation, or result closure.
3. Inventory the required source inputs. Check for `source-event.md`, `captured-steps.md`, `normalized-procedure.md`, `existing-match-check.md`, `authority-risk.md`, `proof-contract.md`, `secret-safety.md`, and `reuse-fit.md`. If one is absent, record which one blocks validation.
4. Validate source truth. The candidate must cite the source Slice/session, command history, logs, artifacts, user decisions, observed failure handling, and final proof that produced the workflow. Reject inferred steps, reconstructed commands without evidence, or claims copied from memory without source support.
5. Validate procedure completeness and reproducibility. Confirm the proposed procedure states purpose, prerequisites, target environment class, parameters, ordered steps, expected outputs, stop conditions, rollback or recovery posture, maintenance/freshness notes, and explicit failure handling. A future agent must be able to rerun the workflow from the artifact without guessing hidden context.
6. Validate safety and secret-safety evidence. Inspect `secret-safety.md` against the candidate text and source evidence. Block if raw credentials, tokens, private host details, unsafe environment-specific material, unredacted command output, or destructive commands appear without redaction and authority treatment.
7. Validate duplicate and update risk. Use `existing-match-check.md` and `reuse-fit.md` to decide whether this should update an existing runbook/procedure instead of creating a new one. Block duplicates, near-duplicates, context-bound one-offs, vague folk wisdom, and procedure candidates whose durable target would create sprawl.
8. Validate authority and preconditions. Use `authority-risk.md` to identify read/write/execute/deploy/package/live-system authority, required approvals, human owner, and preconditions. If authority is missing or ambiguous, the only valid terminal path is `stop_or_handoff`.
9. Validate expected proof. Use `proof-contract.md` to define what future evidence must prove successful execution: command outputs, logs, tests, screenshots, API responses, database checks, version/source refs, manual confirmation, or explicit blocked proof. The proof order must include what claim each proof supports and what missing proof would block.
10. Validate durable-target boundaries. Mark the target only as a candidate: procedure note, runbook draft, command recipe, proof template, workflow rule, or skill candidate. Procedure candidate acceptance is separate from promotion to runbook, skill, rule, or durable KB.
11. Produce `proof-order-template.md` when the proof contract is coherent. The template must list future-execution claim, required proof sequence, acceptable evidence sources, stop conditions, failure evidence, missing-proof handling, and owner/handoff path. If the proof contract is incoherent, do not produce proof-order-template.md as accepted output; write the blocker instead.
12. Record `proposal_validation_recorded` inside `proof-order-template.md`. The record must include accepted, rejected, or deferred status; source evidence checked; completeness verdict; reproducibility verdict; secret-safety verdict; duplicate/update-risk verdict; authority verdict; proof-order verdict; durable target candidate; terminal state or failure route; and next owner.

## Outputs

Primary allowed write: produce `proof-order-template.md` for the active procedure-capture Slice when the candidate passes the proof-order gate. The `proposal_validation_recorded` gate record is a section inside `proof-order-template.md`, not a second unmanaged artifact.

Allowed statuses are `candidate-ready-for-proposal`, `rejected-duplicate`, `rejected-unsafe`, `rejected-non-repeatable`, `deferred-needs-source`, `deferred-needs-authority`, `deferred-needs-proof-contract`, `routed-to-existing-runbook-update`, and `handoff-ready`.

Allowed manifest terminal state is `ready_for_next_step`. Use it only when source truth, safety, duplicate/update risk, authority, proof order, and durable-target boundaries are all explicitly checked. Use manifest failure route `stop_or_handoff` for missing evidence, unclear authority, unsafe material, duplicate risk, or owner review; do not present that failure route as a successful terminal state.

Forbidden: no target operation command, durable write, runbook promotion, skill activation, source mutation, package install, deployment, or live-system command authority from this skill alone. This skill may prepare proposal evidence; it must not write durable KB, mutate canonical runbooks, finalize a Result, or claim future execution success.

## Verification

Before reporting that validation passed, run the evidence-before-claim gate:

1. Identify the exact proposal-readiness claim.
2. Inspect the current Slice inputs and `proof-contract.md`.
3. Confirm source-event proof, captured-step coverage, normalized step clarity, redaction/secret-safety proof, non-duplicate or update-route evidence, authority/precondition classification, proof-order sufficiency, stop conditions, and future execution verification criteria.
4. State what proof supports the claim, what proof is missing, and which terminal state follows.

The final answer or gate record must be able to answer: What claim is being made? What source evidence supports it? Which proof classes are required later? What evidence is missing? Is the durable target only a candidate? Who owns the next step? If any answer is absent, do not claim the candidate is valid; record `stop_or_handoff`.

## Failure Modes

Block with `stop_or_handoff` when the source event is missing, captured steps are inferred, `proof-contract.md` is absent or incoherent, `secret-safety.md` fails, authority is unclear, the existing-match check points to an update path, duplicate procedure risk remains, preconditions are unstated, failure handling is missing, or future execution proof is underspecified.

Reject when the candidate is unsafe, secret-bearing, duplicate, non-repeatable, unverifiable, too context-bound, or a direct jump from a one-off workflow to an active skill, hard plugin rule, canonical runbook, or durable KB mutation.

Hand off when human approval, domain-owner review, restricted evidence access, runbook-library ownership, durable-domain routing, or capability-authoring review is required. The handoff must name the blocked gate, exact missing artifact or authority, and the next owner; it must not promote, mutate, execute, or overclaim while waiting.
