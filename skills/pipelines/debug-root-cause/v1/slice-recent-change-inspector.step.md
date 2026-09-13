---
id: "slice-recent-change-inspector"
kind: "internal_step"
owner_family: "slice_variant"
pipeline_id: "slice.debug-root-cause"
step_id: "slice-recent-change-inspector"
entry_gate: false
public_trigger: false
native_skill_discovery: false
body_path: "capabilities/instructions/pipelines/slice-variants/debug-root-cause/slice-recent-change-inspector.step.md"
source_manifest: "capabilities/pipelines/slice-variants/debug-root-cause.pipeline.json"
legacy_skill_ref: "slice-recent-change-inspector"
architecture_refs:
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#debug-root-cause-variant"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19"
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#lightweight-and-debug-slice-variants"
---

# Slice Recent Change Inspector

## Overview

This skill turns "what changed recently?" into bounded debug evidence for the Debug/root-cause Slice. Core rule: inspect change history as evidence, not as proof of cause, and stop before hypothesis selection, root-cause declaration, source mutation, deployment, or fix work. It may identify candidate leads, rejected leads, and missing source truth. Do not declare root cause. Do not attempt a fix.

## When to Use

Use this after symptom capture and reproduction/evidence planning identify a need to compare the broken behavior with recent change surfaces. It fits regressions, newly failing tests, unexpected runtime behavior after a merge, dependency or config drift, changed environment variables, or a suspected deploy/runtime change.

Do not use it for initial symptom capture, building the first reproduction, tracing data flow, comparing a known working example, writing a regression test, choosing a fix, or validating a completed fix. If live incident authority or operator action dominates, route to operational or hybrid Slice handling.

## Inputs

Required inputs:

- Active Slice contract showing `slice.debug-root-cause`, selected target, authority boundary, and proof order.
- `symptom.md` with expected behavior, observed behavior, environment, timestamp or report window, and affected surface.
- `reproduction.md` or an unable-to-reproduce record that explains the current evidence basis.
- Current `evidence-log.md` when it exists, so recent-change inspection extends the evidence trail instead of replacing it.

Optional inputs are parent Scope constraints, branch/worktree provenance, logs already authorized for debug use, deploy or runtime metadata already available in the workspace, dependency manifests and lockfiles, config files, schema or migration history, feature-flag state, and user-provided last-known-good or first-known-bad timestamps.

## Source Contract

Grounding:

- `capabilities/pipelines/slice-variants/debug-root-cause.pipeline.json` step `slice-recent-change-inspector`
- `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#debug-root-cause-variant`
- `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#lightweight-and-debug-slice-variants`
- `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6`
- `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19`

The manifest marks this as an optional debug step that always produces the step-owned `git-history.md` receipt and may update `evidence-log.md`. It invokes only `slice-recent-change-inspector`, depends on read-only workspace and git inspection services, and reaches `recent_change_evidence_ready` when recent changes were checked, skipped with a stated reason, or blocked with missing source truth recorded. Planning prose in `evidence-log.md` is an input and cannot complete this step.

## Operating Procedure

1. Read the active debug Slice context: `slice.md`, `symptom.md`, `reproduction.md` or the unable-to-reproduce record, and current `evidence-log.md` if present. Extract the affected surface, timeframe, expected/observed delta, environment, source paths, runtime target, and authority boundary.
2. Define the change window. Prefer explicit timestamps, failing build/run time, last known good revision, release/deploy time, dependency update, user report time, or the smallest commit range named by the Slice. If no window exists, record the gap and choose the narrowest defensible read-only range.
3. Inspect permitted read-only sources only: git status, current branch/worktree provenance, staged or unstaged diffs, recent commits, file history for affected paths, config changes, dependency lockfiles, schema/migration files, feature flags, environment/deploy metadata already available in the workspace, and runtime/log snippets already permitted by the debug contract.
4. Separate facts from interpretation. For each candidate change, record what changed, where it came from, source freshness, why it may matter to the symptom, and what evidence would be needed to connect or reject it. Do not label a change as root cause without later discriminating evidence.
5. Preserve negative evidence. If no relevant change appears, write the inspected sources and range so later steps do not repeat the same search. If source truth is stale, unavailable, credentialed, live-only, or outside authority, record the missing input and stop instead of broadening into unsafe commands.
6. Write the step-owned receipt. Always create `git-history.md`, including when history is unavailable or the check is not relevant to this target. A successful checked/skipped receipt must record `gate: recent_change_inspection_recorded` and `terminal_state: recent_change_evidence_ready`; a blocked receipt must record its distinct blocked state and must not claim those success markers. Record the inspected range and sources, outcome, evidence refs, and terminal state. Then optionally append the concise summary, candidate links, rejected change leads, gaps, and next evidence question to `evidence-log.md`.
7. Stop at the handoff boundary. Route candidate leads to evidence ordering, data-flow tracing, working-example comparison, or hypothesis ledger. Do not declare root cause, do not attempt a fix, and do not use a recent-change match as cause without later discriminating evidence. Do not modify files, install packages, run deployments, perform live-system commands, or choose the fix.

## Outputs

Always produce `git-history.md` as this step's receipt. It must name the range, commands or read-only sources consulted, relevant commits/files/config/dependency changes, rejected leads, freshness, gaps, outcome, and terminal state. For a source-grounded skipped check it carries `gate: recent_change_inspection_recorded` and `terminal_state: recent_change_evidence_ready`; for a blocked check it preserves the exact blocked reason and cannot carry the success pair.

Update `evidence-log.md` with a recent-change section containing the inspected window, source classes, candidate changes, why each matters or does not matter, missing evidence, and terminal state `recent_change_evidence_ready` or a blocked handoff for missing history/runtime/deploy truth.

## Terminal States

- `recent_change_evidence_ready`: the bounded window was inspected, not applicable, or skipped with a source-grounded reason; `git-history.md` records the result.
- `blocked_missing_git_history`: git history, affected path history, branch provenance, or repository source truth is unavailable and cannot be safely inferred.
- `blocked_missing_runtime_or_deploy_truth`: runtime, deploy, feature-flag, or environment state is needed but is outside current authority or unavailable without live-system commands.
- `route_to_next_debug_evidence`: recent-change leads exist, but the next owner is evidence ordering, data-flow tracing, working-example comparison, or the hypothesis ledger.

## Verification

Verify trigger fit by confirming the active Slice is `slice.debug-root-cause`, the symptom or reproduction points to possible recent drift, and the next action is evidence collection rather than a fix. Verify content by checking that every listed change has a source path or command/output reference, every skipped source has a reason, and every causal statement is framed as candidate evidence.

Static validation should pass:

- `node tools/validate-internal-skill-body-quality.mjs --skill slice-recent-change-inspector`
- `node tools/validate-internal-skill-trigger-fixtures.mjs --skill slice-recent-change-inspector`

## Failure Modes

Block when the Slice lacks a symptom/reproduction basis, the change window cannot be bounded, git history is unavailable, affected paths are unknown, required deploy/runtime data needs credentials or live authority, or the requested inspection would require mutation, package installation, deployment, or destructive commands.

Hand off when the evidence points outside this skill: data-flow tracing, working-example comparison, operational incident handling, full design reassessment, or user/team-provided deploy history. Record missing history as a blocker; do not replace it with speculation, stacked guesses, or a root-cause claim before finalizing the recent-change step.

## Forbidden Actions

- Do not declare root cause or write `root-cause.md`; that belongs to `slice-root-cause-decision` after hypotheses and discriminating evidence.
- Do not attempt a fix, choose a fix strategy, write `fix-plan.md`, apply a patch, or create `patch.md`; those belong after root-cause proof and authority.
- Do not treat recency as causality. A nearby commit, config edit, dependency change, deploy, or runtime drift is only candidate evidence until later debug steps prove or reject it.
- Do not self-authorize source mutation, package installation, branch/worktree mutation, deployment, destructive commands, or live-system commands.
- Do not broaden into operational execution when live incident authority, production recovery, or deploy authority dominates; route to the operational or hybrid Slice instead.
