---
id: "slice-procedure-maintenance-and-handoff"
kind: "internal_step"
owner_family: "slice_variant"
pipeline_id: "slice.custom-procedure-capture"
step_id: "slice-procedure-maintenance-and-handoff"
entry_gate: false
public_trigger: false
native_skill_discovery: false
body_path: "capabilities/instructions/pipelines/slice-variants/procedure-capture/slice-procedure-maintenance-and-handoff.step.md"
source_manifest: "capabilities/pipelines/slice-variants/procedure-capture.pipeline.json"
legacy_skill_ref: "slice-procedure-maintenance-and-handoff"
architecture_refs:
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#hybrid-implementation-operation-and-procedure-capture-variants"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19"
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#procedure-and-research-slice-variants"
---

# Procedure Maintenance And Handoff

## Overview

This is a reference adapter skill for `superpowers:using-git-worktrees`, narrowed to the final maintenance and handoff step of `slice.custom-procedure-capture`. Its core rule is: record the procedure-capture result boundary, request maintenance state, preserve deferred cleanup and review work, and shape `handoff.md` or an equivalent object-local handoff record; do not merge, push, deploy, clean up, promote durable runbooks, or activate skills from this step.

Classification: `reference_adapter_skill`.

## When to Use

Use when Runtime selected `slice.custom-procedure-capture` and the active work already has one of these states: procedure proposal written, promotion gate evaluated, result recorded, rejected as insufficient, blocked by proof/authority/secret risk, or left for human review or migration.

Use it when the remaining work is maintenance checkpoint routing, stale projection handling, duplicate/update follow-up, manual review, migration to a durable-domain pipeline, branch or worktree cleanup advice, deferred work, or continuation after context loss.

Do not use it to capture the source event, normalize steps, classify risk, write the procedure proposal, validate the proposal, decide promotion, write the result, execute an operation, follow an existing runbook, author a skill, or perform generic branch/worktree setup.

## Source Contract

Grounding sources are `capabilities/pipelines/slice-variants/procedure-capture.pipeline.json` step `slice-procedure-maintenance-and-handoff`, `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6`, `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s8`, `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19`, `docs/architecture/master-plugin-target-architecture-capability-atom-shard-map.html#pipeline.slice.procedure-capture`, `docs/architecture/master-plugin-target-architecture-part-5-pipeline-fabric.html#pause-handoff-and-recovery`, `docs/architecture/master-plugin-target-architecture-part-5-pipeline-fabric.html#runbook-and-procedure-promotion-trigger`, `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#procedure-capture-slice-variant`, and `docs/architecture/master-plugin-target-architecture-validation-harness-evals.html`.

The manifest step produces `handoff.md` and `deferred.md`, gates on `maintenance_or_handoff_recorded`, fails by `stop_or_handoff`, and reaches `ready_for_next_step` only after a maintenance request or handoff record exists. Reference coverage adapts `superpowers:using-git-worktrees`: verify branch, worktree, and cleanup context from existing evidence or read-only inventory before advising on isolation or cleanup; respect ignored-worktree safety; and treat cleanup as an explicit option requiring proof, owner, target, and authority. This skill records those options; it does not create, checkout, switch, stage, stash, commit, merge, push, delete, remove, deploy, or clean anything.

## Operating Procedure

1. Read the final procedure-capture packet. Include source event, captured steps, normalized procedure, existing match check, authority and risk note, proof contract, secret safety check, reuse fit, procedure proposal, promotion gate, result, prior deferred list, prior handoff, and artifact registry/provenance notes when present.
2. Confirm the result boundary before maintenance. A procedure is only a candidate until `result.md` or the equivalent result record says otherwise. Name the highest validated truth without upgrading it: procedure candidate, runbook proposal, rejected/not-now, duplicate or update route needed, blocked by missing proof, blocked by authority, blocked by secret risk, deferred, or promoted only by a later approved durable-domain step.
3. Build or request a read-only workspace/repo/worktree inventory. Use existing recorded evidence when present; otherwise request or capture non-mutating facts only: workspace path, repo path, branch name, worktree path, uncommitted modified files, untracked files, ignored worktree status, source/result refs, and whether any cleanup candidate exists. Do not stage, stash, reset, checkout, switch, merge, push, delete, remove, deploy, or run cleanup.
4. Request or record maintenance checkpoints. Cover result presence, variant shape, stale projection, front-door or index sync, proof validation, promotion readiness, duplicate runbook risk, cleanup readiness, secret-safety review, and procedure-to-skill promotion safety. Each checkpoint must be marked requested, current, stale, blocked, rejected, or owner-review-needed.
5. Handle stale, duplicate, or unsafe artifacts. Put unresolved stale projection, obsolete proposal, duplicate runbook, unsafe command recipe, missing source proof, unredacted secret, uncommitted/untracked inventory risk, or migration follow-up into `deferred.md` with owner, reason, source basis, cleanup candidate, promotion target if any, and resume trigger. Do not delete or rewrite the stale artifact from this skill.
6. Adapt branch and worktree discipline into handoff language. If a source branch, temporary worktree, patch bundle, dirty repo, untracked file, or cleanup candidate remains, record path/ref if known, proof prerequisites, owner, risk, and cleanup options. Cleanup is blocked unless merge/result/deploy proof, owner approval, and exact target are recorded; otherwise emit a proof request or archive proposal.
7. Shape `handoff.md` for the next actor. Include Slice identity, parent/result links, highest validated truth, proposal/rejection/promotion state, maintenance checkpoint table, read-only inventory, branch/worktree cleanup options, deferred items, secret redaction status, forbidden claims, next actor, exact next action, expected return evidence, stale-after condition, and allowed resume step.
8. Set the terminal route. Return `ready_for_next_step` when `handoff.md` or `deferred.md` records the maintenance state and the next actor/action is clear. Return `stop_or_handoff` when result proof, promotion authority, source basis, safe redaction, maintenance source truth, inventory, cleanup ownership, or next actor is missing.
9. Preserve side-effect boundaries. Do not mutate durable KB, canonical runbooks, active skills, indexes, branches, worktrees, source repos, deployment targets, live systems, package state, or maintenance ledgers from this skill. A runbook/procedure/skill promotion can only be proposed or handed to the owning durable-domain or skill-authoring route.

## Outputs

`handoff.md` or the equivalent object-local handoff record must contain the current procedure-capture state, highest validated truth, proof refs, proposal or promotion boundary, maintenance checkpoint statuses, read-only repo/worktree inventory, next actor, next action, expected evidence to paste back, resume trigger, stale-after rule, and forbidden claims.

`deferred.md` is required when unresolved review, migration, duplicate/update route, stale projection, cleanup candidate, branch/worktree decision, uncommitted or untracked inventory risk, rejected proposal, missing proof, missing authority, or secret-risk remediation remains. Each item must include owner, source basis, required proof, cleanup or promotion target, and whether it is a user action, future Slice, maintenance request, durable-domain handoff, or skill-authoring handoff.

These outputs may recommend review, migration, branch cleanup, worktree cleanup, stale artifact retirement, runbook update, no-promote, archive proposal, proof request, or skill-candidate follow-up. They must not perform those actions.

## Verification

Trigger verification must select this skill only after procedure proposal, promotion gate, result, review, rejection, blocked, or deferred state exists and the open work is maintenance or handoff. It must reject early procedure-capture steps and generic git/worktree setup or cleanup tasks.

Content verification checks that the output names the highest validated truth, preserves the proof/result boundary, records maintenance checkpoint state, captures read-only workspace/repo/worktree hygiene including uncommitted and untracked inventory, preserves stale/deferred artifacts with cleanup ownership, distinguishes branch/worktree cleanup advice from authorized git operations, and blocks automatic procedure-to-skill promotion. It must not contain self-authorized branch creation, checkout, switch, stage, stash, commit, merge, push, deletion, worktree removal, cleanup execution, durable runbook mutation, durable KB mutation, skill activation, maintenance execution, deployment, or live commands.

Run:
- `node tools/validate-internal-skill-body-quality.mjs --skill slice-procedure-maintenance-and-handoff`
- `node tools/validate-internal-skill-trigger-fixtures.mjs --skill slice-procedure-maintenance-and-handoff`

## Failure Modes

Stop or hand off when result state is missing, proposal proof is absent, promotion authority is unclear, source event references are missing, secret-safety status is unresolved, existing-match status is stale, duplicate runbook risk is unresolved, read-only repo/worktree inventory is missing, cleanup target/owner/proof is missing, maintenance checkpoints cannot be requested, or the next actor is unknown.

Route back to event extraction, step normalization, or proposal writing when no procedure proposal exists. Route back to validation when completeness, safety, duplicate risk, evidence coverage, or future execution clarity has not been checked. Route back to promotion gate when durable mutation, runbook write, or skill-candidate activation is being requested. Route to durable-domain handoff when review approves later runbook, KB, protocol, or operations-knowledge mutation. Route to a user handoff when branch/worktree cleanup, stale artifact retirement, untracked-file disposition, or migration action requires authority this skill does not hold.

## Quick Reference

| State | Route |
| --- | --- |
| Maintenance state recorded and next actor clear | `ready_for_next_step` |
| Human review, migration, or cleanup remains | `handoff.md` and usually `deferred.md` |
| Branch or worktree cleanup candidate remains | option only; no git operation here |
| Uncommitted or untracked files remain | inventory plus owner action; no stash/stage/delete here |
| Stale or duplicate artifact remains | `deferred.md` with owner and source basis |
| Missing proof, authority, redaction, or next actor | `stop_or_handoff` |
