---
id: "tect-result-writer"
kind: "internal_step"
owner_family: "slice_variant"
pipeline_id: "slice.full-design-to-execution"
step_id: "tect-result-writer"
entry_gate: false
public_trigger: false
native_skill_discovery: false
body_path: "capabilities/instructions/pipelines/slice-variants/full-design-to-execution/tect-result-writer.step.md"
source_manifest: "capabilities/pipelines/slice-variants/full-design-to-execution.pipeline.json"
legacy_skill_ref: "tect-result-writer"
architecture_refs:
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#full-development-slice-variant"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19"
---

# Tect Result Writer

## Overview

This skill writes the Full Design-To-Execution Slice `result.md` artifact from the highest validated truth available at closure time. Core rule: a result may state only what the Slice contract, proof evidence, authority, and freshness support; every stronger desired claim becomes missing proof, residual risk, deferred work, forbidden claims, or handoff.

## When to Use

Use this only after the full-development Slice has reached the `slice-result-writer` / `result_gate` boundary: design/spec work is complete or explicitly blocked, plan and execution posture are known, verification has fresh evidence or a blocker, and deployment/live/handoff posture has been decided.

Select this skill when the next artifact is `result.md` and the agent must distinguish local proof, deployment proof, live proof, user/team handoff, rollback, deferred work, supersession, or blocked state.

Do not use it to brainstorm, interrogate components, write an implementation plan, execute tasks, run tests, deploy, collect live evidence, select a durable-domain target, clean a worktree or branch, merge, update indexes, execute maintenance, or repair stale artifacts. Route those steps back to their owning skill, service, validator, or handoff.

## Source Contract

Ground this procedure in `capabilities/pipelines/slice-variants/full-design-to-execution.pipeline.json`: `capability_surface.skills.tect-result-writer`, step `slice-result-writer`, gate `result_gate`, terminal states `result_boundary_ready` and `blocked_missing_result`, and atom `pipeline.slice.full_design_to_execution.result.writer`.

Architecture anchors are `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#full-development-slice-variant`, `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#result-promotion-pipeline`, `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6`, and `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19`.

Reference bodies are absorbed as source material, not invoked as canonical behavior: brainstorming contributes problem framing, options, and tradeoff checks; the custom spec pipeline contributes decisions directory, component extraction, dimension sweep, human decision, README, cross-cutting review, reconcile finding/decision handling, updated spec, implementation-ready spec, synthesis, and synthesis handoff checks; writing/executing plans contribute implementation plan, task, checkpoint, execute, and verification boundaries; verification-before-completion contributes proof before claim; subagent-driven development contributes subagent review and handoff evidence; git-worktree and branch finishing contribute worktree, branch, cleanup, finish, and merge boundary checks.

Required inputs are the parent Slice front door, selected `slice.full-design-to-execution` variant contract, artifact contract, proof contract, authority state, freshness state, implementation plan, execution record, verification record, deployment-validation or handoff record, live-validation verdict when required, decisions directory status, cross-cutting review/reconciliation/synthesis status, deferred notes, promotion signals, and transition history.

## Operating Procedure

1. Confirm closure readiness. If the Slice lacks a parent, selected variant, artifact contract, proof contract, closure request, or result authority, stop with `blocked_missing_result` and route to entry, contract, verification, deployment gate, or handoff.
2. Sweep upstream completeness before truth classification: design problem framing and tradeoff are preserved; decisions directory/README status is current; component extraction and dimension sweep are complete or blocked; human decisions are resolved or named; cross-component review findings are clean, reconciled, deferred, or blocker-owned; the synthesis handoff produced an implementation-ready spec before planning.
3. Audit plan/execution evidence. Treat the implementation plan, executed tasks, subagent reports, review results, checkpoints, changed files, branch/worktree state, cleanup, finish, or merge status as inputs only. They never self-certify completion.
4. Audit proof evidence against the declared proof contract. Separate local proof, deployment proof, live proof, user/team handoff proof, rollback proof, stale evidence, fixture-only validation, generated projections, and unsupported claims. Fresh verification is required before any positive claim.
5. Classify the highest validated truth without upgrades. Local verification supports local truth only; deployment request is not deployment proof; deployment proof is not live proof; a promotion candidate is not durable truth; old CI, branch existence, stale memory, or fixture success is not current proof unless the contract explicitly accepts it.
6. Select exactly one terminal state: `completed_local_verified`, `completed_deploy_verified`, `completed_live_verified`, `completed_user_handoff`, `completed_with_deferred_work`, `blocked_missing_proof`, `blocked_missing_authority`, `rolled_back_verified`, `superseded_by_followup_slice`, or `promoted_to_durable_domain`.
7. Write `result.md` using this shape: terminal state; highest_validated_truth; proof_basis with links or command/log summaries; authority_basis; freshness_basis; missing_proof; forbidden_claims; residual_risk; deferred_work; promotion_edge_status; follow_up_slice_candidates; handoff_next_action; front-door/index/maintenance check requested.
8. Preserve forbidden claims explicitly. If the desired closeout says fixed, complete, deployed, live, safe, promoted, merged, cleaned up, or done beyond evidence, list the claim and the exact missing proof, authority, freshness, or owner.
9. Route residue without executing it: deferred work to `deferred.md`, durable learning to promotion/deferred routing, stale shape to maintenance check request, user-owned proof to handoff, new component work to follow-up Slice, and branch/worktree/merge/cleanup work to finishing or git-worktree owners.
10. Stop after the result packet or proposal. This skill may write the Slice result artifact only when the caller already has canonical write authority; it does not run commands, mutate source repos, deploy, live probe, update durable-domain storage, perform promotion, clean branches, merge, or execute maintenance.

## Outputs

Primary output is a concise `result.md` artifact or result-packet proposal for the parent Slice. It must include `highest_validated_truth`, `terminal_state`, evidence/proof links, authority and freshness notes, `missing_proof`, `forbidden_claims`, `residual_risk`, `deferred_work`, `promotion_edge_status`, follow-up Slice candidates, handoff owner, and exact next action.

Downstream artifacts may be named but not silently performed: `deployment-validation.md`, `live-validation.md`, `deferred.md`, `promotion.md`, `handoff.md`, `recovery-notes.md`, a transition record, front-door/index update check, or maintenance check request. The output must keep plan execution, deployment execution, fixture validation, subagent report, branch state, worktree cleanup, finish, and merge evidence distinct from final completion proof.

## Verification

Before claiming the result boundary is ready, re-read the proof contract and compare every result statement to fresh evidence. Reject these forbidden upgrades explicitly: local proof upgraded to live proof, plan execution treated as completion, deployment request treated as deployment proof, deployment proof upgraded to live proof, fixture success treated as real validation, promotion proposal treated as accepted durable truth, branch/worktree cleanup treated as product proof, or stale/generated projection treated as current truth.

Validate this skill with `node tools/validate-internal-skill-body-quality.mjs --skill tect-result-writer` and trigger coverage with `node tools/validate-internal-skill-trigger-fixtures.mjs --skill tect-result-writer`. Also confirm the owned fixtures parse as JSON and that scoped whitespace checks pass for this skill and its two fixtures.

## Failure Modes

Use `blocked_missing_proof` when evidence is absent, stale, fixture-only, generated-only, from the wrong target, contradicted, or insufficient for the requested terminal state. Use `blocked_missing_authority` when result writing, deployment, live checking, promotion, rollback, branch cleanup, merge, or handoff needs authority that is absent.

Use `completed_user_handoff` when the user or team owns the remaining proof action and the current result can only state what is ready for them. Use `completed_with_deferred_work` only when deferred work is explicit and does not invalidate the supported truth. Use `superseded_by_followup_slice` when new scope owns the remaining closure. Use `promoted_to_durable_domain` only for the spine-owned transition edge after governed promotion acceptance; the durable-domain write remains external.

If design/spec decisions are incomplete, cross-cutting review has unresolved contradictions, reconciliation is pending, synthesis is absent, implementation-ready spec is missing, verification did not run, deployment/live proof is required but absent, or the parent Slice contract is missing, do not invent a result. Write the blocker or handoff route and return to the owning step.
