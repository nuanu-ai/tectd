---
id: "slice-op-prep-promotion-router"
kind: "internal_step"
owner_family: "slice_variant"
pipeline_id: "slice.operational-preparation"
step_id: "slice-op-prep-promotion-router"
entry_gate: false
public_trigger: false
native_skill_discovery: false
body_path: "capabilities/instructions/pipelines/slice-variants/operational-preparation/slice-op-prep-promotion-router.step.md"
source_manifest: "capabilities/pipelines/slice-variants/operational-preparation.pipeline.json"
legacy_skill_ref: "slice-op-prep-promotion-router"
architecture_refs:
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#operational-preparation-and-operational-execution-variants"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19"
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#operational-and-hybrid-slice-variants"
---

# Operational Preparation Promotion Router

## Overview
This skill routes reusable or deferred candidates from an operational preparation package after the package has reached prep-only closure. Its core rule is routing only: preserve candidates, blockers, owners, and no-promotion reasons without writing durable truth, accepting promotion, executing the operation, or claiming the operation completed.

## When to Use
Use this in `slice.operational-preparation` after `result.md` records `prepared_not_executed` truth and the prep package has enough `handoff.md`, `proof-contract.md`, `operation-plan.md`, rollback posture, and evidence to decide what should survive.

Typical triggers are a reusable runbook_candidate, a messy procedure_capture candidate, durable_kb or durable_ops_kb seed, operations_candidate, devops_infra learning, security_candidate, protocol_candidate, product_candidate, stale or missing operational documentation, a follow_up_slice, a blocked promotion idea, or an explicit no_promotion_recorded decision.

Do not use it before result writing, proof contract, or handoff readiness. Do not use it for operational execution, live validation, rollback, canonical runbook or KB writes, DevOps source mutation, maintenance repair, accepted promotion, or cleanup.

## Source Contract
Ground this behavior in `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#operational-preparation-and-operational-execution-variants`, `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#operational-and-hybrid-slice-variants`, `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6`, and `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19`.

The owning manifest is `capabilities/pipelines/slice-variants/operational-preparation.pipeline.json`. The step anchor is `step_graph.steps.slice-op-prep-promotion-router`; it invokes this skill, produces `promotion.md` and `deferred.md`, gates `promotion_candidate_or_no_promotion_recorded`, ends as `promotion_routed` or `escalated_to_procedure_capture`, and fails through `defer_or_request_authority`. Atom anchors are `pipeline.slice.operational-preparation` and `pipeline.slice.operational_preparation.promotion.router`. No external skill body is referenced for this step.

## Operating Procedure
1. Load the closure packet: Slice identity, operation target, authority boundary, current-state baseline, risk-impact notes, preflight checks, command plan, rollback plan, proof contract, handoff, `result.md`, residual risk, and existing deferred notes. If `result.md` does not record prep-only truth, route back to result writing.
2. Check the highest validated truth. Treat the package as prepared_not_executed even when the command plan is exact. Do not infer live success, deploy proof, rollback proof, or canonical durable knowledge from preparation artifacts.
3. Extract durable signals and assign one primary route per item. Use `runbook_candidate` for repeatable commands/checklists, `procedure_candidate` or `procedure_capture` for messy workflows, `durable_kb` or `durable_ops_kb` for stable operating facts, `operations_candidate` for incident/queue/cadence lessons, `devops_infra` for environment/config/service topology notes, `security_candidate` for access/secret/risk controls, `protocol_candidate` for integration or wire-contract observations, `product_candidate` for user-facing/product decision fallout, `follow_up_slice` for new bounded work, `deferred` for blocked but useful items, or `no_promotion` when nothing should survive.
4. Keep each item at candidate altitude. Candidate routing may name a later durable KB, runbook, DevOps, operations, security, protocol, product, or procedure owner, but it must not write or accept that owner's canonical object.
5. For every candidate, record source artifacts, evidence refs, proof_level, freshness, authority_readiness, sensitivity, target_hint, duplicate or stale-match risk, blocked reason, and next_owner_or_step. Weak one-off observations become no_promotion_recorded or deferred rather than promotion candidates.
6. Decide readiness without doing downstream work. Ready means proof basis, owner, target scope, authority need, and sensitivity handling are clear enough for a later Result / Promotion or stateful-domain owner to review. Procedure candidates may route to procedure capture, but this skill does not normalize the procedure or promote it.
7. Shape `promotion.md` for ready candidates and explicit no-promotion decisions. Shape `deferred.md` for missing proof, missing authority, unclear owner, stale evidence, duplicate risk, sensitive material, unresolved handoff work, or follow-up Slice candidates.
8. Preserve the boundary in the routing packet: no durable write, no accepted promotion, no execution, no live action, no rollback, no maintenance repair, no branch/worktree cleanup, and no claim that the operation ran.

## Outputs
The output is `promotion.md`, `deferred.md`, or both for the selected operational-preparation Slice. Candidate records should include `candidate_id`, `candidate_type`, `source_artifacts`, `evidence_refs`, `proof_level`, `freshness`, `authority_readiness`, `sensitivity`, `target_hint`, `routing_decision`, `blocked_or_deferred_reason`, `next_owner_or_step`, and `no_promotion_reason` when applicable.

Candidate types should remain explicit: runbook, durable KB, DevOps infra, operations, security, protocol, product, procedure, follow-up Slice, deferred, or no-promotion. If one prep artifact suggests several destinations, split candidate records instead of hiding ownership ambiguity inside one route.

Valid routing postures include `promotion_routed`, `escalated_to_procedure_capture`, `deferred_to_follow_up`, `blocked_missing_proof`, `blocked_missing_authority`, `blocked_unclear_owner`, `blocked_sensitive_evidence`, and `no_promotion_recorded`. These records may feed Result / Promotion, procedure capture, runbook review, operations knowledge, DevOps infra review, security review, protocol review, product review, maintenance readiness, handoff, or a follow-up Slice, but they do not create canonical truth.

## Verification
Verify selection by checking that the active variant is operational preparation, `result.md` exists, the result truth is prepared_not_executed, and the immediate work is candidate routing rather than command execution, result writing, durable-domain mutation, or maintenance repair.

Static validation is `node tools/validate-internal-skill-body-quality.mjs --skill slice-op-prep-promotion-router` and `node tools/validate-internal-skill-trigger-fixtures.mjs --skill slice-op-prep-promotion-router`. Content review must confirm the body names the owning manifest, the Part 6B operational anchors, final map `#s6` and `#s19`, `promotion_candidate_or_no_promotion_recorded`, `promotion.md`, `deferred.md`, the required durable KB, runbook, DevOps, operations, security, protocol, product, procedure, follow-up, and no-promotion route classes, and the no durable write, no accepted promotion, and no execution boundary.

## Failure Modes
Block when the selected variant, operation target, result truth, proof contract, handoff, source evidence, authority boundary, or current-state basis is missing. Route backward when the Slice still needs result writing, proof-contract building, handoff packaging, preflight shaping, command planning, or rollback planning before promotion routing can be honest.

Defer when a candidate may be useful but lacks proof, owner, freshness, authority, sensitivity handling, duplicate review, or target scope. Escalate to procedure capture when the durable value is a messy process that must be normalized and checked before any runbook or skill candidate exists. Route to operational execution or hybrid when the user grants mutation authority or code/config work is required.

Stop if asked to write a canonical runbook, update durable operations knowledge, edit DevOps infrastructure docs, accept a promotion, execute commands, validate live state, repair maintenance drift, clean git/worktree state, hide deferred work, or claim the prepared operation completed. Record no_promotion_recorded when evidence review leaves no survivor.
