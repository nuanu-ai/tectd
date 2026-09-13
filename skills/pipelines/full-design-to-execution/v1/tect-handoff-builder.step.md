---
id: "tect-handoff-builder"
kind: "internal_step"
owner_family: "slice_variant"
pipeline_id: "slice.full-design-to-execution"
step_id: "tect-handoff-builder"
entry_gate: false
public_trigger: false
native_skill_discovery: false
body_path: "capabilities/instructions/pipelines/slice-variants/full-design-to-execution/tect-handoff-builder.step.md"
source_manifest: "capabilities/pipelines/slice-variants/full-design-to-execution.pipeline.json"
legacy_skill_ref: "tect-handoff-builder"
architecture_refs:
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#full-development-slice-variant"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19"
---

# Tect Handoff Builder

## Overview
This standalone executable skill builds the Full Design-To-Execution Slice handoff or closure packet. Its core rule is that a future actor must be able to resume from artifacts without old chat context, while the packet must not upgrade proof, authority, cleanup, promotion, or completion claims beyond validated Slice evidence.

## When to Use
Use this at the `slice-handoff-builder` step of `slice.full-design-to-execution` when work is paused, blocked, context-overflowed, transferred to another agent, handed to a user or team, or ready to close with a final continuation record. Select it after verification, deployment/live-validation routing, result writing, promotion/deferred routing, and maintenance checks have either established the current truth level or produced an explicit blocker.

Do not use it to implement work, run tests, deploy, perform live validation, write the result, promote durable knowledge, clean branches, merge work, delete a worktree, push, create a PR/MR, or resolve design decisions. Route those actions to the owning verification, deployment, result, promotion, maintenance, git/worktree, branch-finish, or spec-pipeline skill.

## Source Contract
Grounding sources are `capabilities/pipelines/slice-variants/full-design-to-execution.pipeline.json` step `slice-handoff-builder`, `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#full-development-slice-variant`, `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6`, `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19`, and the atom row `pipeline.slice.full_design_to_execution.handoff.builder` in `docs/architecture/master-plugin-target-architecture-capability-atom-shard-map.html`.

Classification is `skill_body`. The manifest terminal states for this step are `handoff_required`, `completed_with_handoff`, and `closed`; the broader Slice completion contract also preserves states such as `implemented_locally`, `completed_local_verified`, `completed_deploy_verified`, `completed_live_verified`, `blocked_missing_proof`, `blocked_missing_authority`, and `deferred_to_follow_up`. The same manifest separately references `superpowers:finishing-a-development-branch`; that reference is policy-limited source material or a separate runtime invocation, not the canonical behavior of this Tect skill. This skill may produce `handoff.md` content or a handoff update proposal, but it does not mutate source, execute pipelines, persist active pipeline state, run deployment, issue live-system commands, promote durable knowledge, merge branches, push work, delete worktrees, or perform cleanup.

## Operating Procedure
1. Confirm the trigger, audience, and terminal intent. Name whether this is pause, blocked proof, blocked authority, user-managed action, team transfer, context overflow, follow-up Slice routing, cleanup handoff, or final closure. Identify the parent Program/Epoch/Scope/Slice, selected variant, current lifecycle state, and next accountable owner.
2. Load the Slice artifact set named by the manifest: `README.md`, `slice.md`, `design-spec.md`, `decisions/`, `cross-cutting-review.md`, `implementation-ready-spec.md`, `implementation-plan.md`, `execution.md`, `verification.md`, `deployment-validation.md`, `result.md`, `promotion.md`, `deferred.md`, plus optional `handoff.md`, `maintenance.md`, `review.md`, `evidence/`, `logs/`, `screenshots/`, and `patches/` where present. Treat missing expected artifacts as gaps; do not invent their contents from memory.
3. Classify the highest validated truth and proof state from artifacts only: local proof, deployment proof, live proof, user/team handoff, blocked proof gap, blocked authority, deferred follow-up, promoted, no-promote, or closed. Preserve stale, historical, derived, and unverified evidence labels.
4. Gather open blockers and authority gaps. For each gap, name the missing proof, missing approval, unavailable environment, stale artifact, unresolved decision, branch/worktree uncertainty, or missing owner; then route it to the owning verification, deployment/live-validation, result, promotion/deferred, maintenance, git/worktree, or spec-pipeline step.
5. Capture branch/worktree status without changing it. Record branch name, base/provenance if known, worktree path, dirty or untracked files, staged status, PR/MR/review state, cleanup candidates, and destructive actions still forbidden. If branch or worktree status was not refreshed, label it unknown and require a refresh before merge, push, cleanup, or branch deletion.
6. Capture result/promotion/deferred state. Record whether `result.md` exists and what proof level it claims, whether `promotion.md` chooses promote or no-promote, which deferred items remain, which follow-up Slice or durable-domain route owns them, and what residual risks or rejected work must not disappear.
7. Build restart context. Point to the first artifact a future actor must read, commands already run, commands deliberately not run, environment or service assumptions, stale-after or refresh-before-use conditions, exact next action, next owner, required inputs, and the return-proof contract needed to move from `handoff_required` to `completed_with_handoff` or `closed`.
8. Write the packet with these headings: target and audience; terminal-state candidate; current state and highest validated truth; proof already captured and proof still missing; blockers, authority gaps, and failure routes; branch/worktree status and cleanup requirements; result/promotion/deferred state; restart context; next owner and next action; return-proof contract; forbidden actions and forbidden claims.
9. Keep the handoff compact and artifact-targeted. Prefer a `handoff.md` content block or final response packet when artifact writing is authorized by the parent Runtime; otherwise return the proposed handoff content and state that no canonical source was updated.
10. Before releasing the packet, scan for overclaims: local proof described as live, deployment request described as deployment proof, old chat context treated as source truth, missing owner, missing return-proof contract, hidden deferred work, hidden cleanup requirement, or a branch/deploy/promotion/cleanup instruction without authority.

## Outputs
The output is a handoff or closure packet for the active Full Design-To-Execution Slice. It must include target object, audience, current state, highest validated truth, proof state, terminal-state candidate, next owner, next action, required source artifacts to read first, proof already captured, proof still missing, authority gaps, explicit forbidden claims, open blockers, failure routes, branch/worktree status, cleanup requirements, result/promotion/deferred state, maintenance notes, restart context, return-proof contract, and freshness limits.

When the packet targets `handoff.md`, include the content ready for that artifact plus whether the current run actually had authority to write it. If source writes are not authorized, the output is a proposed packet only. It must not perform implementation, verification, deployment, promotion, result writing, git cleanup, branch mutation, or live validation.

## Verification
Verify the body by checking that the handoff references `capabilities/pipelines/slice-variants/full-design-to-execution.pipeline.json`, preserves `docs/architecture/*.html` source grounding, and uses only the seven Layer 6B sections. Verify the packet by tracing every claim to a Slice artifact or named missing artifact; every proof statement to local, deployment, live, user/team, blocked, deferred, promoted, no-promote, or closed evidence; every next action to an owner; every cleanup requirement to branch/worktree status; every terminal state to the manifest or completion contract; and every incomplete proof to a forbidden claim.

The final packet is acceptable only when it has no ownerless next action, no hidden blocker, no unsupported terminal state, no source or branch mutation, no deployment or promotion action, and no result overclaim. Deterministic checks are `node tools/validate-internal-skill-body-quality.mjs --skill tect-handoff-builder` and `node tools/validate-internal-skill-trigger-fixtures.mjs --skill tect-handoff-builder`.

## Failure Modes
Block or emit a handoff-only packet when the target Slice cannot be identified, required artifacts are missing, the highest validated truth is unclear, proof is stale, authority is absent, branch/worktree status is unknown, cleanup requirements are unsafe, the next owner is unnamed, or the return-proof contract is missing.

Route back to result writing when no result boundary exists, to deployment/live validation when proof is required but absent, to promotion/deferred routing when residual work is uncategorized, to maintenance when artifact shape or front-door/index consistency is stale, to git/worktree or branch-finish handling when cleanup is requested, and to spec-pipeline skills when decisions or synthesis remain unresolved. Never close the Slice from a handoff if unresolved blockers, forbidden claims, authority gaps, cleanup hazards, missing proof, or ownerless actions remain.
