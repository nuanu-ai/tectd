---
id: "slice-procedure-promotion-gate"
kind: "internal_step"
owner_family: "slice_variant"
pipeline_id: "slice.custom-procedure-capture"
step_id: "slice-procedure-promotion-gate"
entry_gate: false
public_trigger: false
native_skill_discovery: false
body_path: "capabilities/instructions/pipelines/slice-variants/procedure-capture/slice-procedure-promotion-gate.step.md"
source_manifest: "capabilities/pipelines/slice-variants/procedure-capture.pipeline.json"
legacy_skill_ref: "slice-procedure-promotion-gate"
architecture_refs:
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#procedure-and-research-slice-variants"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19"
---

# Procedure Promotion Gate

## Overview
This skill decides whether a procedure-capture proposal is eligible to continue toward promotion review. It is an Tect-owned gate for `slice.custom-procedure-capture`, not a wrapper around another skill and not a durable-domain writer.

The core rule is: record a promotion candidate only when proof, authority, duplicate status, secret safety, reuse fit, target lane, and candidate completeness are explicit. This step may write the Slice-local `promotion-gate.md`; it must not write canonical durable storage, approve promotion, scrub secrets itself, create an active skill, or mutate a runbook, KB page, index, source repo, deployment target, or live system.

## When to Use
Use this in `slice.custom-procedure-capture` after source event extraction, captured step normalization, existing-match checking, authority-risk classification, proof contract creation, secret safety review, reuse-fit evaluation, durable target selection, validation, proposal writing, and optional skill-candidate routing are available.

Use it when a `procedure-proposal.md`, `runbook-draft.md`, `command-recipe.md`, proof template, or `skill-candidate.md` is being considered for later durable review and the Slice needs a go/no-go eligibility verdict before any source-of-truth artifact can change.

Do not use it to start procedure capture, normalize steps, search for duplicates, scrub secrets, choose the target lane, write the proposal, approve promotion, edit durable KB/runbook/procedure files, create an active skill, execute the procedure, deploy, migrate, or mutate live systems. If the user or runtime asks for those actions, route to the owning prior step, stateful domain pipeline, adoption/skill-authoring flow, operational workflow, or human/runtime approval path.

## Source Contract
Ground this skill in `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html` section 10, `docs/architecture/master-plugin-target-architecture-part-6c-stateful-domain-pipelines.html`, `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6`, `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19`, and `docs/architecture/master-plugin-target-architecture-capability-atom-shard-map.html` row `pipeline.slice.procedure_capture.promotion.gate`.

The owning manifest is `capabilities/pipelines/slice-variants/procedure-capture.pipeline.json`, step `slice-procedure-promotion-gate`. The manifest marks this required approval step as invoking `skill:slice-procedure-promotion-gate`, producing `promotion-gate.md`, gating on `promotion_candidate_recorded_without_durable_write`, failing by `stop_or_handoff`, and continuing only through terminal state `ready_for_next_step`.

Read only the active procedure-capture packet and its prior-step artifacts: `source-event.md`, `captured-steps.md`, `normalized-procedure.md`, `existing-match-check.md`, `authority-risk.md`, `proof-contract.md`, `secret-safety.md`, `reuse-fit.md`, validation output, `procedure-proposal.md`, optional drafts, and target-lane notes. Part 6C owns durable-domain mutation: Durable KB and Runbook Library pipelines decide accepted canonical writes through their own promotion request loaders, authority/freshness gates, canonical write gates, provenance edges, indexes, and result writers.

## Operating Procedure
1. Verify candidate completeness. Require source event, ordered steps, proposal body, target lane, source/proof references, existing-match verdict, authority-risk record, proof contract, secret-safety verdict, reuse-fit verdict, and validation outcome. Route back to the missing prior step when any required artifact is absent.
2. Check proof adequacy. Confirm the proof contract names future execution evidence, expected terminal state, failure handling, and review evidence needed for the target lane. Block if the proposal depends on unproven claims, stale source truth, missing command outputs, unverified live behavior, or evidence that cannot be cited.
3. Check authority and risk. Confirm that the authority-risk record identifies read/write/execute/deploy/promote risk, required approver, allowed mutation location, rollback or deprecation posture where relevant, and whether agent action is allowed. If the lane needs human/runtime approval, record that approval as pending; do not substitute this gate for approval.
4. Check existing-match and reuse status. Accept continuation only when `existing-match-check.md` supports a safe route: `no_match` with bounded negative findings, `partial_match` with an update/merge target, or `stale_or_superseded_match` with a refresh/supersession target. Block on unresolved `exact_match`, duplicate-stockpile risk, or `conflict` unless the next owner is explicitly reuse, update, maintenance, or human review.
5. Check secret and safety posture. Confirm `secret-safety.md` says the candidate contains no secrets, raw tokens, private credentials, unsafe host details, unredacted logs, or environment-specific steps that would teach unsafe access. Block if sensitive material remains or redaction changed the proof enough that review cannot trust it. This step verifies the scrubber result; it does not perform the scrub.
6. Check target lane fit. Verify the candidate lane is one of the allowed proposal lanes such as procedure note, runbook update, command recipe, proof-order template, durable KB seed, runbook library intake, or skill-authoring candidate. Reject direct one-off-to-plugin-rule jumps; skill authoring can only be a later candidate route with evidence and reuse fit.
7. Decide the gate verdict. Use `eligible_for_promotion_review` when all checks pass and approval is still future-facing. Use `review_required` when the candidate can continue only to a human/runtime review queue. Use `blocked_missing_evidence`, `blocked_authority`, `blocked_duplicate_or_conflict`, `blocked_secret_safety`, `blocked_wrong_lane`, or `not_ready_candidate` when a check fails.
8. Write `promotion-gate.md`. Include the candidate identity, target lane, proof verdict, authority verdict, existing-match/reuse verdict, secret-safety verdict, completeness checklist, final eligibility verdict, required approver or next owner, explicit statement that no durable write occurred, and remaining blockers or residual risks.

## Outputs
The required artifact write is the Slice-local `promotion-gate.md`. It records a promotion candidate without durable write only when the final verdict is `eligible_for_promotion_review` or `review_required`, the gate `promotion_candidate_recorded_without_durable_write` is true, and the next terminal state is `ready_for_next_step`.

The output must include proof, authority, existing-match/reuse, secret-safety, target-lane, candidate completeness, terminal state, and handoff routing sections, plus a decision table showing pass, fail, or review-required for each gate. It may recommend a next owner such as promotion review, runbook update review, maintenance, human approval, durable-domain pipeline intake, skill-authoring intake, or return to an earlier procedure-capture step.

Terminal states are `ready_for_next_step` after an eligible or review-required gate, and `stop_or_handoff` after any blocker. This skill never writes the durable runbook, procedure, KB page, command recipe, proof template, skill, index, or front door. It also never marks the proposal as promoted; later review/result owners record promotion, rejection, update, or handoff truth.

## Verification
Run `node tools/validate-internal-skill-body-quality.mjs --skill slice-procedure-promotion-gate` and `node tools/validate-internal-skill-trigger-fixtures.mjs --skill slice-procedure-promotion-gate`. Also parse both owned fixture JSON files, scan the skill body for exactly the seven Layer 6B H2 sections, and run scoped whitespace and diff checks on this skill plus its two fixture files.

For content verification, inspect `promotion-gate.md`: it must cite the prior procedure-capture artifacts, evaluate proof, authority, existing-match, secret-safety, target lane, and completeness, and state that no durable write occurred. Confirm that passing the gate means only eligibility for review, not accepted promotion, durable truth, active skill creation, or source mutation.

## Failure Modes
Return `stop_or_handoff` when prior procedure-capture artifacts are missing, proof is not reproducible, source evidence is stale or uncitable, authority is unclear, required approval or authorization is absent, duplicate status is unresolved, a conflict exists, secret safety is not clean, the target lane is invalid, or the proposal is too incomplete for review.

Block rather than promote when the candidate contains credentials, raw tokens, private environment details, command output that exposes unsafe access, a one-off workflow being forced into a plugin rule, an exact existing artifact that should be reused, or a partial/stale artifact that needs update or maintenance before any new durable object exists.

Route away when the request belongs to proposal writing, durable-domain editing, source mutation, skill authoring, deployment, migration, live-system action, final result writing, or maintenance repair. Preserve the failed check and next owner so a later review can repair, reject, or safely continue the candidate.
