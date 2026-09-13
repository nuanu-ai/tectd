---
id: "slice-debug-promotion-router"
kind: "internal_step"
owner_family: "slice_variant"
pipeline_id: "slice.debug-root-cause"
step_id: "slice-debug-promotion-router"
entry_gate: false
public_trigger: false
native_skill_discovery: false
body_path: "capabilities/instructions/pipelines/slice-variants/debug-root-cause/slice-debug-promotion-router.step.md"
source_manifest: "capabilities/pipelines/slice-variants/debug-root-cause.pipeline.json"
legacy_skill_ref: "slice-debug-promotion-router"
architecture_refs:
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#debug-root-cause-variant"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19"
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#lightweight-and-debug-slice-variants"
---

# Slice Debug Promotion Router

## Overview

This skill classifies what should happen after a debug/root-cause Slice has written its Result. Its core rule is that promotion routing is only a handoff decision: reusable learning, regression, procedure, durable-knowledge, and follow-up candidates may be named, but durable truth, execution, deployment, cleanup, and final closure stay with their owning pipeline and proof gates.

## When to Use

Use this after `slice-debug-result-writer` has recorded the highest validated debug truth from `symptom.md`, `reproduction.md`, `evidence-log.md`, `hypotheses.md`, `root-cause.md`, `fix-plan.md` or `no-fix-result.md`, `verification.md`, and `result.md`. Typical triggers are a repeated diagnosis path, a missing regression-proof target, a reusable debugging procedure, a stale KB correction, a runbook or proof-order candidate, an operations lesson, or a follow-up Slice that should not be hidden inside the completed debug work.

Do not use it to find root cause, apply fixes, verify the symptom, write `result.md`, deploy, run live commands, clean branches, write canonical KB/runbook/domain content, or claim that a promotion was accepted.

## Source Contract

Ground this skill in `capabilities/pipelines/slice-variants/debug-root-cause.pipeline.json` step `slice-debug-promotion-router`, which produces `promotion.md` and `deferred.md`, gates on `promotion_candidate_or_no_promotion_recorded`, and ends as `promotion_routed` or `fix_deferred_to_followup`.

Architecture anchors are `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#debug-root-cause-variant`, `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#lightweight-and-debug-slice-variants`, `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6`, `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19`, and `docs/architecture/master-plugin-target-architecture-capability-atom-shard-map.html#pipeline.slice.debug_root_cause.promotion.router`.

Because this step mentions promotion, also apply `docs/architecture/master-plugin-target-architecture-part-6c-stateful-domain-pipelines.html`: a Slice or Result may request promotion, while stateful-domain pipelines own canonical storage, freshness, provenance, indexes, and accepted/rejected promotion results.

## Operating Procedure

1. Load the debug closure packet: parent Slice identity, selected debug variant, authority state, `root-cause.md`, `verification.md`, `result.md`, residual risks, deferred items, unresolved defects, and any user/team handoff constraints. If `result.md` or proof state is missing, route back to the result or verification step.
2. Check the highest validated truth before routing anything. Local proof can support local learning candidates; it cannot imply deployment, live behavior, canonical knowledge, or accepted promotion. Preserve unresolved defects and missing proof as blockers or follow-up candidates.
3. Classify each signal into one routing class: reusable debugging lesson, regression test/proof gap, procedure or proof-order candidate, runbook update, stale durable-KB correction, protocol/product/security/DevOps/operations knowledge candidate, follow-up Slice, no-promotion decision, or blocked candidate.
4. For every candidate, record the source evidence, proof level, freshness, authority requirement, sensitivity/risk, target hint, owner if known, and why the candidate is reusable or necessary. Weak or one-off observations become `no_promotion` or `deferred_missing_evidence`.
5. Choose the next route without performing it. Candidate routes may include Result / Promotion target selection, stateful-domain promotion request, procedure capture, runbook-library review, operations-knowledge review, or a follow-up Slice proposal.
6. Write `promotion.md` when at least one candidate is ready for downstream routing. Write `deferred.md` when candidates exist but are blocked by missing proof, missing authority, owner ambiguity, stale evidence, sensitive material, or unresolved defects.
7. State the boundary explicitly: no durable-domain write, no accepted-promotion claim, no deploy or live-system action, no branch/worktree cleanup, no maintenance repair, and no closure beyond the already proven debug Result.

## Outputs

The output is a routing packet in `promotion.md`, `deferred.md`, or both. It should include candidate records with `candidate_id`, `candidate_type`, `source_artifacts`, `evidence_refs`, `proof_level`, `freshness`, `authority_need`, `target_hint`, `routing_decision`, `blocked_reason`, and `next_owner_or_step`.

Valid terminal posture is `promotion_routed`, `fix_deferred_to_followup`, `no_promotion_recorded`, `blocked_missing_proof`, or `blocked_missing_authority`. The packet may feed Result / Promotion, stateful-domain pipelines, procedure capture, maintenance readiness checks, or handoff builders, but this skill does not write canonical truth or mark downstream promotion accepted.

## Verification

Verify the skill was selected only after the debug Result exists and the task is candidate routing, not diagnosis, fixing, verification, or durable writing. Check that every candidate points back to current debug evidence and that unresolved defects are visible as blockers or follow-up work.

Static validation is `node tools/validate-internal-skill-body-quality.mjs --skill slice-debug-promotion-router` and `node tools/validate-internal-skill-trigger-fixtures.mjs --skill slice-debug-promotion-router`. Content review should confirm the body references the debug manifest, the Part 6B debug anchors, final map `#s6` and `#s19`, the atom row, and the Part 6C durable-domain boundary; trigger review must include at least two positive routing scenarios and one non-trigger.

## Failure Modes

Block when `result.md`, proof state, authority state, or the selected debug variant is missing. Defer when a candidate is useful but stale, sensitive, ownerless, contradicted, or lacks enough evidence to route safely. Route back to debug verification when the original symptom is not proven, and to a follow-up Slice when new implementation, redesign, ops, or research work is required.

Stop immediately if the requested action is to write KB/runbook/domain truth, accept a promotion, deploy, run live checks, clean branches or worktrees, hide unresolved defects, or close the Slice by saying promotion work is done. Those actions belong to downstream Result / Promotion, stateful-domain, maintenance, operation, git/worktree, or handoff owners.

If candidates conflict, keep all conflicting claims visible and route to the owner that can reconcile them; do not collapse contradictory evidence into a clean promotion story. If no candidate survives review, record `no_promotion_recorded` with the reviewed evidence and reason so later maintenance does not rediscover the same rejected path.

The operational safety invariant is zero direct writes, zero accepted-promotion claims, and zero cleanup or live-action shortcuts from this router.
