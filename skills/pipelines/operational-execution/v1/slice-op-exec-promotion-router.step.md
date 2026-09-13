---
id: "slice-op-exec-promotion-router"
kind: "internal_step"
owner_family: "slice_variant"
pipeline_id: "slice.operational-execution"
step_id: "slice-op-exec-promotion-router"
entry_gate: false
public_trigger: false
native_skill_discovery: false
body_path: "capabilities/instructions/pipelines/slice-variants/operational-execution/slice-op-exec-promotion-router.step.md"
source_manifest: "capabilities/pipelines/slice-variants/operational-execution.pipeline.json"
legacy_skill_ref: "slice-op-exec-promotion-router"
architecture_refs:
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#operational-preparation-and-operational-execution-variants"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19"
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#operational-and-hybrid-slice-variants"
---

# Slice Operational Execution Promotion Router

## Overview

This skill routes lessons and candidates after an authorized operational execution has reached its result boundary. Its core rule is classification and handoff only: preserve what should survive beyond the Slice, why it is ready or blocked, and which owner should review it, without writing durable storage or claiming promotion accepted.

## When to Use

Use this after `slice-op-exec-result-writer` has recorded `result.md` for `slice.operational-execution` and the next question is whether the operation produced durable knowledge, a runbook or procedure update, DevOps/infra knowledge, operations knowledge, security knowledge, protocol knowledge, deferred follow-up, or an explicit no-promotion decision.

Typical triggers include a new or changed operation, risky manual action, repeated recovery path, corrected runbook step, missing proof template, live-system trap, rollback or observation lesson, authority gap, stale operational knowledge, or post-action finding that should be routed to a domain owner.

Do not use this to execute commands, validate post-action state, write `result.md`, run rollback, open a hidden second operation, update canonical KB/runbooks/protocol/security/DevOps pages, accept durable promotion, repair indexes, clean branches or worktrees, or advance truth beyond the proof already recorded.

## Source Contract

Ground this behavior in `capabilities/pipelines/slice-variants/operational-execution.pipeline.json` step `slice-op-exec-promotion-router`, which is required, invokes `slice-op-exec-promotion-router`, produces `promotion.md` and `deferred.md`, gates on `promotion_candidate_or_deferred_recorded`, ends at `ready_for_next_step`, and fails to `stop_or_handoff`.

Architecture anchors are `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#operational-preparation-and-operational-execution-variants`, `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#operational-and-hybrid-slice-variants`, `docs/architecture/master-plugin-target-architecture-part-6c-stateful-domain-pipelines.html`, `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6`, and `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19`. Part 6C supplies the hard storage boundary: `domain_pipeline_owns_canonical_storage`, canonical mutation gates, promotion result writers, and stateful-domain pipelines own accepted durable truth after this router hands off a request. Atom and mapping anchors are `pipeline.slice.operational-execution`, `pipeline.slice.operational_execution.promotion.router`, `capabilities/pipelines/slice-variants/operational-execution.pipeline.json#step_graph.steps.slice-op-exec-promotion-router`, and `capabilities/pipelines/slice-variants/operational-execution.pipeline.json#step_graph.steps.slice-op-exec-promotion-router.invokes.slice-op-exec-promotion-router`.

## Operating Procedure

1. Load the closure packet: Slice identity, operation target, authority-confirmation, current-state baseline, preflight, risk-stop-conditions, action-ledger, execution-log, post-action-validation, observation-window if present, rollback or recovery notes, `result.md`, residual risk, and existing deferred items. If `result.md` or proof state is missing, route back to result writing or post-action validation.
2. Check the highest validated truth before classifying any candidate. Use only the evidence already recorded by the operational execution Slice; do not upgrade a handed-off, blocked, partial, or locally inferred state into live or durable truth.
3. Extract durable signals and classify each as one primary candidate type: `durable_kb` for general durable findings, `runbook_library` for reusable commands or proof-order recipes, `devops_infra` for deploy/runtime/config/service topology facts, `operations_knowledge` for triage cadence, ownership, handoff, monitoring, and operating lessons, `security_knowledge` for authority, credential, access, unsafe pattern, or sensitive-evidence concerns, `protocol_knowledge` for external API/protocol/chain/SDK behavior and constraints, `procedure_capture` for messy repeated process evidence, `skill_authoring` for mature repeatable agent behavior, `follow_up_slice` for new work, or `no_promotion` when nothing should survive.
4. For every candidate, record source artifacts, exact evidence references, proof level, freshness, authority/readiness, environment or version scope, sensitivity, target hint, duplicate or stale-match signals, and why the candidate should or should not survive beyond this Slice.
5. Decide readiness. Mark ready only when proof, authority, source scope, sensitivity handling, and next owner are clear enough for downstream review. Mark deferred or blocked when proof is missing, authority is absent, the owner is unclear, evidence is stale, the finding is too environment-specific, secrets are embedded, observation is still open, or a duplicate runbook/domain page may already exist.
6. Route without doing the target owner's work. Ready candidates can go to Result / Promotion review, a stateful-domain promotion request, runbook-library review, procedure capture, operations or DevOps knowledge review, security/protocol review, skill authoring intake, maintenance readiness, user handoff, or follow-up Slice proposal. Accepted canonical storage remains with the selected stateful-domain pipeline, not this operational execution Slice.
7. Create `promotion.md` for ready candidates and explicit no-promotion decisions. Create `deferred.md` for blocked candidates, unresolved follow-up, missing authority, missing proof, stale or sensitive evidence, unclear owner, duplicate-risk review, or observation-window work. Preserve the boundary: no canonical durable write, no accepted-promotion claim, no operation, no rollback, no live check, no cleanup, and no maintenance repair.

## Outputs

The output is `promotion.md`, `deferred.md`, or both. Candidate records should include `candidate_id`, `candidate_type`, `source_artifacts`, `evidence_refs`, `proof_level`, `freshness`, `authority_readiness`, `environment_scope`, `sensitivity`, `target_hint`, `routing_decision`, `blocked_or_deferred_reason`, `next_owner_or_step`, and `no_promotion_reason` when applicable.

Valid routing postures are `promotion_routed`, `deferred_to_follow_up`, `no_promotion_recorded`, `blocked_missing_proof`, `blocked_missing_authority`, `blocked_unclear_owner`, `blocked_sensitive_evidence`, `blocked_stale_or_duplicate_candidate`, and `stop_or_handoff`. These records may feed Result / Promotion, stateful-domain pipelines, runbook review, procedure capture, maintenance readiness, or handoff builders, but they do not create canonical knowledge or mark downstream promotion accepted.

## Verification

Verify selection by checking that the Slice variant is operational execution, `result.md` exists, and the immediate task is post-result routing rather than execution, validation, rollback, result writing, durable-domain mutation, or maintenance repair. Confirm every candidate cites current operational evidence, proof level, authority/readiness, and target scope, and that every unresolved item remains visible as deferred, blocked, follow-up, or no-promotion.

Static validation is `node tools/validate-internal-skill-body-quality.mjs --skill slice-op-exec-promotion-router` and `node tools/validate-internal-skill-trigger-fixtures.mjs --skill slice-op-exec-promotion-router`. Content review must confirm this body names the operational-execution manifest step, `promotion_candidate_or_deferred_recorded`, `promotion.md`, `deferred.md`, final map `#s6` and `#s19`, the Part 6B operational anchors, durable target classes, authority/readiness checks, and the no durable write, no accepted promotion, no live action, no cleanup boundary.

## Failure Modes

Block when `result.md`, proof state, authority state, operation target, selected variant, or current evidence references are missing. Route backward when the Slice still needs post-action validation, observation, rollback or recovery notes, residual risk capture, or result writing before promotion routing can be honest. Keep fuzzy ownership visible instead of converting it into a clean candidate.

Defer when a candidate may be useful but lacks proof, authority, freshness, owner assignment, sensitivity handling, duplicate-match review, domain readiness, or observation-window closure. Record `no_promotion_recorded` when no durable candidate survives review, including the evidence checked and reason.

Stop immediately if asked to write canonical KB, update a runbook, change DevOps/security/protocol/operations storage, accept durable promotion, execute a command, run a live check, roll back, repair maintenance drift, clean git/worktree state, or hide deferred operational work. Those actions belong to downstream domain, Result / Promotion, operation, maintenance, git/worktree, or handoff owners with their own gates.
