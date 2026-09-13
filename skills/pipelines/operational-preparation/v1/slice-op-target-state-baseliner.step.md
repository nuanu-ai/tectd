---
id: "slice-op-target-state-baseliner"
kind: "internal_step"
owner_family: "slice_variant"
pipeline_id: "slice.operational-preparation"
step_id: "slice-op-target-state-baseliner"
entry_gate: false
public_trigger: false
native_skill_discovery: false
body_path: "capabilities/instructions/pipelines/slice-variants/operational-preparation/slice-op-target-state-baseliner.step.md"
source_manifest: "capabilities/pipelines/slice-variants/operational-preparation.pipeline.json"
legacy_skill_ref: "slice-op-target-state-baseliner"
architecture_refs:
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#operational-and-hybrid-slice-variants"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19"
---

# Target State Baseliner

## Overview

This skill establishes the pre-operation state of an operational target so later risk, preflight, command planning, rollback, handoff, and proof can compare against a known baseline. The core rule is: record current evidence, expected target state, and safety gaps in `current-state.md`; do not mutate the target or claim the operation is complete.

## When to Use

Use this skill when an operational-preparation Slice needs `current-state.md` before risk modeling, preflight, command planning, rollback planning, or handoff. It fits prep-only requests such as deploy, seed, redeploy, rollback, data repair, service restart, chain/API operation, or infrastructure change where the user wants an exact operation package but has not authorized execution. The trigger is strongest when the package needs target identity, current repo/worktree/branch state, clean/dirty status, remote/protected branch awareness, service or data state, approval needs, and rollback implications captured before commands are planned.

Do not use it to execute the operation, run mutating commands, create or switch branches, create worktrees, install dependencies, deploy, migrate, seed, delete, restart, or promote a runbook. If execution authority exists now, route to the operational execution baseline or execution pipeline. If root cause is unknown, route to debug first.

## Source Contract

- Architecture: `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6` defines Slice/result proof boundaries; `#s19` selects operational prep when the signal is an ops target with no execution; `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#operational-preparation-and-operational-execution-variants` requires `current-state.md` and forbids target mutation during preparation.
- Manifest: `capabilities/pipelines/slice-variants/operational-preparation.pipeline.json` step `slice-op-target-state-baseliner` produces `current-state.md`, gates on `current_state_recorded_or_gap_declared`, fails with `record_missing_baseline`, and reaches `baseline_ready`.
- Atom/map anchors: `pipeline.slice.operational_preparation.op.target.state.baseliner`, `pipeline.slice.operational-preparation`, `service.git_worktree_anchor`, and `capabilities/services/workspace-map/service.json` ground the workspace, git, and worktree read surface.
- Reference adapter skill for `superpowers:using-git-worktrees`: adapt its branch/worktree isolation and safety ideas into read-only baseline capture. Do not copy its creation workflow or run its setup/install/test commands.

Read only the sources needed for the baseline: `operation-intent.md`, `authority-boundary.md`, `slice.md`, parent Scope/Program current state, workspace map, repo/worktree inventory, relevant runbooks/env maps, service docs, prior incidents, and already-authorized read-only evidence.

## Operating Procedure

1. Confirm the active Slice is `slice.operational-preparation`, the requested target is explicit, and the authority boundary is prep/read-only. If target, environment, expected target state, or authority is ambiguous, stop with `record_missing_baseline`.
2. Record target identity first: service/system name, environment, account/cluster/host/repo/chain/database as applicable, user-facing surface, current owner, intended final state, and non-goals. Keep this distinct from the desired command sequence, which belongs later in `slice-op-command-plan-builder`.
3. Declare the command safety boundary before collecting evidence. Allowed read-only commands may inspect status, refs, logs, config, schema, health, or dry-run output when already authorized. Forbidden mutating commands include deploy, write, delete, seed, migrate, restart, rollback, branch creation or switching, worktree creation, dependency installation, package execution, or any command that changes target state.
4. Identify baseline surfaces from the operation intent: code/repo, package/version, configuration, database, infrastructure, service health, queues/jobs, chain state, API state, logs, external dependencies, and the source files that define expected behavior. Keep irrelevant surfaces out of `current-state.md`.
5. Load workspace-map and git/worktree evidence in read-only mode. Capture workspace root, source repo, selected worktree path, current branch, upstream or remote, current commit/ref, divergence if known, clean/dirty state, untracked artifact notes, protected branch risk, active worktree ownership, and cleanup obligations that could affect handoff. Do not create, switch, remove, or clean a branch or worktree.
6. For each target surface, record source class and freshness: live read, repo file, config map, dashboard/log excerpt, prior result, memory-derived clue, or missing. Historical clues are not current baseline unless refreshed by an allowed read-only check.
7. Capture the actual baseline facts: current version/ref, deployed or intended environment, key configuration values without secrets, database/schema or migration posture, infrastructure/service identifiers, health/status, queue/backlog posture, chain/API/log state where relevant, and known incident/degraded states.
8. Build the operational baseline judgment without planning commands: risk list, approval needs, rollback implications, cleanup implications, stale evidence, gaps, and assumptions. This is not the full risk model or rollback plan; it is the input those later skills must consume.
9. If a fact requires a mutating command, privileged target operation, hidden credential, dependency install, branch switch, worktree creation, deployment, migration, seed, restart, or write, do not perform it. Record the missing fact, required actor, and whether `slice-op-dry-run-or-readonly-validator` should handle an allowed check later.
10. Write `current-state.md` as a comparison baseline and handoff to `slice-op-command-plan-builder`, `slice-op-risk-and-impact-modeler`, `slice-op-preflight-check-builder`, `slice-op-rollback-plan-builder`, and `slice-op-proof-contract-builder`. Separate confirmed facts from gaps, assumptions, stale evidence, and blocked checks.
11. End in `baseline_ready` only when every required surface is either recorded with source/freshness or explicitly declared as a gap. Otherwise end with `record_missing_baseline` and name the missing source, approval, or authority.

## Outputs

Produce `current-state.md` with these sections:

- Target identity and environment.
- Expected target state and non-goals from `operation-intent.md`.
- Command safety boundary with allowed read-only commands and forbidden mutating commands.
- Source files read and evidence table with source class, freshness, and comparison value.
- Current version, configuration, database, infrastructure, service health, chain/API/log state, and dependency posture as applicable.
- Workspace/git/worktree/branch snapshot: workspace map source, repo, worktree path, branch, upstream or remote, commit/ref, clean/dirty state, untracked notes, protected branch risk, and cleanup obligations.
- Risk list, approval needs, rollback implications, cleanup implications, gaps, assumptions, stale evidence, and required follow-up checks.
- Handoff to `slice-op-command-plan-builder` and other downstream preparation steps, with a final baseline verdict.

Allowed terminal states are `baseline_ready` and `record_missing_baseline`. The output must say "prepared baseline" or "baseline gap declared", never "operation completed".

## Verification

Verify the body with `node tools/validate-internal-skill-body-quality.mjs --skill slice-op-target-state-baseliner` and trigger scenarios with `node tools/validate-internal-skill-trigger-fixtures.mjs --skill slice-op-target-state-baseliner`. For actual use, inspect `current-state.md` against the manifest gate: each relevant target surface has either current evidence, source/freshness, and comparison value, or a named gap with owner/next check. Confirm target identity, expected target state, command safety boundary, git/worktree clean/dirty and remote/protected branch posture, approval needs, risk list, rollback implications, cleanup obligations, and command-plan handoff are present. Also verify no secrets, branch/worktree mutation, dependency install, deployment, migration, seed, restart, write, or operation-completed claim appears in the baseline.

## Failure Modes

Block with `record_missing_baseline` when the operation target, expected target state, environment, authority boundary, workspace root, repo/worktree, branch, upstream or remote, clean/dirty state, protected branch posture, or evidence source cannot be identified. Block or hand off when current evidence is stale and cannot be refreshed read-only, when required facts depend on privileged or mutating commands, when secret material would need to be exposed, when approvals are missing, or when branch/worktree cleanup risk makes the target unsafe to plan from.

Route to debug when the current state is unknown because a failure has no root cause. Route to operational execution only after explicit execution authority and required preflight gates exist. Route to command planning only after `current-state.md` gives the future actor enough baseline evidence, gaps, approvals, risks, rollback implications, and cleanup notes to plan safely without re-discovering the target.

## Quick Reference

Input: prep-only operation target. Output: `current-state.md`. Gate: `current_state_recorded_or_gap_declared`. Terminal states: `baseline_ready` or `record_missing_baseline`.
