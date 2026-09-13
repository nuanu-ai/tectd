---
id: "slice-lightweight-promotion-router"
kind: "internal_step"
owner_family: "slice_variant"
pipeline_id: "slice.lightweight-tdd-development"
step_id: "slice-lightweight-promotion-router"
entry_gate: false
public_trigger: false
native_skill_discovery: false
body_path: "capabilities/instructions/pipelines/slice-variants/lightweight-tdd/slice-lightweight-promotion-router.step.md"
source_manifest: "capabilities/pipelines/slice-variants/lightweight-tdd.pipeline.json"
legacy_skill_ref: "slice-lightweight-promotion-router"
architecture_refs:
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#lightweight-development"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19"
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#lightweight-and-debug-slice-variants"
---

# Slice Lightweight Promotion Router

## Overview

This skill decides what, if anything, should be routed after a lightweight TDD Slice result. Its core rule is that lightweight promotion is post-result routing only: it records candidates, no-promotion, or deferral without running TDD, patching code, verifying the change, writing `result.md`, writing durable truth, accepting promotion, hiding missing proof, or extending the Slice lifecycle.

## When to Use

Use this after `slice-lightweight-result-writer` has recorded `result.md` from a bounded lightweight Slice with `slice.md`, `tdd-notes.md` or `test-plan.md`, `implementation-notes.md`, and `verification.md`. Typical triggers are a reusable small-change procedure, stale local or durable knowledge found during the work, a missing runbook or proof-order note, a follow-up Slice that should remain visible, a blocked promotion candidate, or a decision that no promotion is warranted.

Do not use it to choose tests, perform TDD, patch source, verify the change, decide deploy impact, write the final result, create canonical KB/runbook/domain content, accept a promotion, clean git state, or claim completion beyond the proof already recorded.

## Source Contract

Ground this skill in `capabilities/pipelines/slice-variants/lightweight-tdd.pipeline.json` step `slice-lightweight-promotion-router`, which produces `promotion.md` and `deferred.md`, gates on `promotion_candidate_or_no_promotion_recorded`, and ends as `promotion_routed` or `deferred_to_follow_up`.

Architecture anchors are `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#lightweight-development`, `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#lightweight-and-debug-slice-variants`, `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6`, and `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19`. Atom and manifest anchors are `pipeline.slice.lightweight-tdd`, `capabilities/pipelines/slice-variants/lightweight-tdd.pipeline.json#step_graph.steps.slice-lightweight-promotion-router`, and `capabilities/pipelines/slice-variants/lightweight-tdd.pipeline.json#step_graph.steps.slice-lightweight-promotion-router.invokes.slice-lightweight-promotion-router`.

When a candidate mentions durable knowledge, runbooks, domain content, or promotion, preserve the Part 6C durable-domain boundary from `docs/architecture/master-plugin-target-architecture-part-6c-stateful-domain-pipelines.html`: the Slice or Result may request promotion, but canonical storage, indexes, freshness, repair, and domain-specific routing belong to the downstream domain pipeline. This skill may route a request; it must not write canonical durable truth or reusable runbooks directly from the Slice result.

## Operating Procedure

1. Load the lightweight closure packet: Slice identity, selected lightweight variant, `slice.md`, TDD or test-plan notes, implementation notes, verification proof, result truth, residual risk, deferred notes, source artifacts, and authority state. If `result.md` or verification proof is absent, route back to result writing or verification.
2. Check whether the completed change stayed lightweight. If architecture ambiguity, repeated failures, deploy/live proof, or broad follow-up work became necessary, record a deferred route to full, debug, hybrid, operational, or follow-up Slice ownership instead of stretching the lightweight lifecycle.
3. Classify each possible routing signal as reusable procedure, stale knowledge correction, missing runbook or proof-order note, durable knowledge candidate, follow-up Slice, deferred item, no-promotion decision, or blocked candidate.
4. Apply the proof, freshness, and authority gates. Every candidate needs source artifacts, evidence references, proof level, freshness state, owner or target hint, authority need, and a reason it is reusable enough to survive beyond this Slice. One-off observations with no durable value become `no_promotion`.
5. Choose the next-step route without performing it. Valid routes include Result/Promotion review, stateful-domain promotion request, procedure capture, runbook review, maintenance readiness, deploy-impact or hybrid handoff, operational handoff, or follow-up Slice proposal.
6. Create `promotion.md` when at least one candidate is ready for downstream review or when a no-promotion decision must be explicit. Create `deferred.md` when useful candidates are blocked by missing proof, missing authority, unclear owner, stale evidence, sensitive material, deploy/live dependency, or unresolved work.
7. Preserve the boundary in the packet: no source patch, no test run, no verification run, no `result.md` write, no durable-domain write, no accepted-promotion claim, no deploy or live action, no branch/worktree cleanup, no maintenance repair, and no hidden second implementation lifecycle.

## Outputs

The output is `promotion.md`, `deferred.md`, or both. If no candidate survives review, `promotion.md` may contain only the explicit no-promotion record. Candidate records should include `candidate_id`, `candidate_type`, `source_artifacts`, `evidence_refs`, `proof_level`, `freshness`, `authority_need`, `target_hint`, `routing_decision`, `blocked_reason`, and `next_owner_or_step`.

Manifest terminal states are `promotion_routed` and `deferred_to_follow_up`; failure or non-promotion states are `no_promotion_recorded`, `blocked_missing_proof`, `blocked_missing_authority`, `blocked_stale_or_sensitive_evidence`, and `blocked_deploy_or_live_dependency`. The manifest failure route is `defer_or_request_authority`. The packet may feed Result/Promotion, stateful-domain pipelines, procedure capture, maintenance readiness, deploy-impact, hybrid or operational handoff, or follow-up builders, but it does not create canonical knowledge or mark downstream promotion accepted.

## Verification

Verify this skill was selected only after a lightweight TDD Result exists and the current task is routing, deferral, or no-promotion classification. This verification is a routing-packet completeness check, not a test run, implementation check, deploy probe, live check, or result rewrite. Check that each candidate points back to current Slice evidence, proof level, freshness, and authority state, and that unresolved work remains visible as blocked or follow-up routing.

Static validation is `node tools/validate-internal-skill-body-quality.mjs --skill slice-lightweight-promotion-router` and `node tools/validate-internal-skill-trigger-fixtures.mjs --skill slice-lightweight-promotion-router`. Content review must confirm the body names the lightweight manifest step, the Part 6B lightweight anchors, final map `#s6` and `#s19`, the atom/manifest anchors, `promotion_candidate_or_no_promotion_recorded`, `promotion.md`, `deferred.md`, and no durable write or accepted-promotion authority.

## Failure Modes

Block when `result.md`, verification proof, authority state, source artifact pointers, or selected lightweight variant context is missing. Defer when a candidate is useful but lacks proof, has no clear owner, depends on live/deploy evidence, contains sensitive material, has stale evidence, or reveals work too broad for the lightweight Slice.

Route backward when the change has not been verified, route sideways to deploy-impact or hybrid flow when live proof affects truth, route to the appropriate stateful-domain intake when durable promotion is only a candidate, and route forward to a follow-up Slice when new implementation or investigation is required. Stop if asked to patch code, write `result.md`, write KB/runbook/domain truth, accept promotion, deploy, run live checks, clean branches or worktrees, bury unresolved work, or treat local proof as live proof.

If no candidate survives review, record `no_promotion_recorded` with the evidence checked and reason. The invariant is explicit routing or explicit no-routing, never silent promotion debt; zero silent routing debt is the closing rule.
