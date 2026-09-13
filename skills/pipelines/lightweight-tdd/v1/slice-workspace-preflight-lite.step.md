---
id: "slice-workspace-preflight-lite"
kind: "internal_step"
owner_family: "slice_variant"
pipeline_id: "slice.lightweight-tdd-development"
step_id: "slice-workspace-preflight-lite"
entry_gate: false
public_trigger: false
native_skill_discovery: false
body_path: "capabilities/instructions/pipelines/slice-variants/lightweight-tdd/slice-workspace-preflight-lite.step.md"
source_manifest: "capabilities/pipelines/slice-variants/lightweight-tdd.pipeline.json"
legacy_skill_ref: "slice-workspace-preflight-lite"
architecture_refs:
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#lightweight-and-debug-slice-variants"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19"
---

# Lightweight Workspace Preflight

## Overview

This reference adapter skill adapts `superpowers:using-git-worktrees` into Tect's Lightweight TDD Slice path. Its core rule is simple: before test selection or source edits, prove repo/worktree identity, branch provenance, dirty state, allowed target files, authority to edit, isolation posture, and cleanup obligations, or stop before mutation.

## When to Use

Use this after immediate context is loaded and before test target selection or TDD edits when the selected variant is `slice.lightweight-tdd-development`. It fits small understood code, config, or business-rule changes with clear acceptance checks where the next question is whether the current workspace is safe enough for local TDD.

Use it when the current directory might be the workspace control repo instead of the source repo, branch reuse authority is unclear, untracked or dirty files may collide with the Slice, allowed target files are not yet named, or an existing worktree might need cleanup tracking.

Do not use it for full design work, unknown-root-cause debugging, deployment/live validation, branch cleanup, branch/worktree creation, package setup, dependency installation, or post-edit verification. Route those to the full, debug, hybrid, git/worktree support, setup, verification, or user handoff path.

## Source Contract

- Record ID: `tect-skill.slice-lightweight-debug.slice-workspace-preflight-lite`
- Pipeline manifest: `capabilities/pipelines/slice-variants/lightweight-tdd.pipeline.json`
- Resolution state: `tect_skill`
- Manifest step: `slice-workspace-preflight-lite`, a required Lightweight TDD service step that invokes this Tect skill and the `superpowers:using-git-worktrees` reference.
- Gate: `workspace_safe_or_authority_declared`; success terminal state: `preflight_ready`; failure route: `request_git_worktree_support_or_block`.
- Architecture: `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6`, `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19`, `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#lightweight-and-debug-slice-variants`, `docs/architecture/master-plugin-target-architecture-part-6d-cross-cutting-support-services.html#git-worktree`, and `docs/architecture/master-plugin-target-architecture-part-6d-cross-cutting-support-services.html#workspace-map`.
- Atom: `pipeline.slice.lightweight_tdd.workspace.preflight.lite`, which checks branch/worktree/repo dirtiness and whether lightweight work needs isolation.
- External reference: `skills/references/superpowers/using-git-worktrees/SKILL.md`; adapted for read-only preflight only, not copied and not used as authority to create worktrees.

## Operating Procedure

1. Confirm preconditions. The selected variant must be `slice.lightweight-tdd-development`, the change must be small, acceptance checks must be clear, and the next step must be test target selection or TDD edits. If any condition is false, stop and route to the appropriate heavier variant.
2. Confirm allowed target files and authority to edit. List the exact files, directories, or generated artifacts the Slice may touch. If the target files are missing, too broad, owned by another active Slice, or outside the user's authority, emit `blocked_missing_authority` before any edit.
3. Classify repo/worktree identity from workspace-map evidence or current path context: workspace control repo, child source repo, execution worktree, generated/cache/temp root, external research clone, or unknown. Parent workspace git status is never source repo truth.
4. Read branch provenance for the explicit target root: branch name, detached state, remote, upstream, intended base, ahead/behind, merge-base confidence, protected branch exposure, and explicit branch reuse authority. If remote freshness is required but unavailable, block rather than fetching.
5. Read repo and worktree dirtiness: staged, unstaged, untracked, ignored-but-relevant artifacts, generated outputs, nested repo boundaries, multi-repo overlap, and whether another active Slice or worktree owns the same surface.
6. Decide isolation posture. A clean execution worktree with matching scope, branch provenance, owned dirtiness, and allowed target files can proceed. Dirty source anchors, protected branches, unknown base, unowned untracked files, or overlapping active work require `request_git_worktree_support_or_block`.
7. Keep branch/worktree creation outside this skill. The pipeline owns branch/worktree creation through the git/worktree support surface after authority gates pass; this skill can request that support or block, but it must not create, switch, remove, clean, or retire branches or worktrees.
8. Check dependency and baseline readiness without installing packages: known test or acceptance target can be selected next, required baseline command is not omitted, no package install or setup mutation is needed, no deploy/live validation is required, and repeated failure has not shifted the Slice into debug.
9. Record cleanup obligation awareness when an execution worktree is used: owner, linked Slice, terminal proof needed before cleanup, and cleanup candidate route. This skill records the obligation only.
10. Emit one route in `workspace-preflight.md`. If target root, branch provenance, allowed target files, authority to edit, dirty-state ownership, isolation, dependency readiness, and cleanup awareness are proven, write the exact lines `gate: workspace_safe_or_authority_declared` and `terminal_state: preflight_ready`, then hand off to TDD cycle. Otherwise emit a blocked state before edits and omit both success lines. There is no mutation beyond preflight.

## Outputs

Produce exactly one primary preflight artifact: `workspace-preflight.md`. Final local verification remains exclusively owned by `slice-lightweight-verification-runner` in `verification.md`. Include:

- target root and repo/worktree identity;
- branch, remote, upstream, intended base, ahead/behind, merge-base confidence, protected-branch verdict, and branch reuse authority;
- dirty and untracked state, including ignored-but-relevant artifacts and ownership;
- allowed target files and authority to edit those files only;
- isolation decision and whether lightweight work needs an existing clean worktree or git/worktree support;
- dependency and baseline readiness without setup mutation;
- cleanup obligation awareness for any execution worktree;
- terminal state: `preflight_ready`, `request_git_worktree_support_or_block`, or `blocked_missing_authority`; a successful artifact includes exact lines `gate: workspace_safe_or_authority_declared` and `terminal_state: preflight_ready`.

`preflight_ready` is only permission to continue to the TDD cycle. It is not implementation proof, verification proof, result closure, deployment proof, cleanup approval, or Slice completion.

## Verification

Content is valid when it names the manifest step, cites architecture HTML sources, adapts worktree, branch, dirty-state, and cleanup behavior from the external reference, and keeps the whole procedure read-only. It must explicitly cover allowed target files, authority to edit, protected branch safety, no mutation beyond preflight, pipeline-owned branch/worktree creation, dependency and baseline readiness without installing packages, and handoff to TDD cycle or blocked state.

Trigger fixtures must include at least two positive lightweight pre-edit scenarios and negative scenarios for post-edit verification, branch/worktree creation, cleanup execution, full design, debug, setup, or live/deploy work. Validate with:

- `node tools/validate-internal-skill-body-quality.mjs --skill slice-workspace-preflight-lite`
- `node tools/validate-internal-skill-trigger-fixtures.mjs --skill slice-workspace-preflight-lite`

## Failure Modes

Block before edits when root classification is unknown, repo/worktree identity is ambiguous, the allowed target files are missing or too broad, authority to edit is absent, the target is dirty without ownership, branch provenance is missing, upstream/base is ambiguous, the branch is protected, another Slice owns the surface, dependency setup would require package installs, baseline proof is not selectable, repeated failures indicate debug, or deploy/live validation is required.

Use `request_git_worktree_support_or_block` when a clean execution worktree is needed but does not already exist, branch reuse lacks authority, protected branch policy blocks edits, or cleanup obligations are unknown. Use `blocked_missing_authority` when the user or Runtime must approve branch reuse, dirty-state handling, allowed target files, authority to edit, or a heavier variant.

Forbidden actions from this skill include fetch, checkout, switch, branch creation, worktree add/remove, reset, clean, stash, push, package install, deployment, live-system commands, source mutation, durable-domain writes, registry edits, ledger edits, and active Runtime state persistence. This skill never grants branch/worktree creation authorization. Block mutating files outside allowed target files. Block starting TDD without workspace preflight when the preflight gate has not emitted `preflight_ready`.

## Quick Reference

Ready means root known, branch provenance known, allowed target files named, edit authority declared, dirty state owned, isolation adequate, setup unchanged, and cleanup awareness recorded. Unsafe means stop before edits and hand off to git/worktree support, a heavier Slice variant, or the user.
