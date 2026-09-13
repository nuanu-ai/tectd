---
id: "slice-lightweight-maintenance-and-handoff"
kind: "internal_step"
owner_family: "slice_variant"
pipeline_id: "slice.lightweight-tdd-development"
step_id: "slice-lightweight-maintenance-and-handoff"
entry_gate: false
public_trigger: false
native_skill_discovery: false
body_path: "capabilities/instructions/pipelines/slice-variants/lightweight-tdd/slice-lightweight-maintenance-and-handoff.step.md"
source_manifest: "capabilities/pipelines/slice-variants/lightweight-tdd.pipeline.json"
legacy_skill_ref: "slice-lightweight-maintenance-and-handoff"
architecture_refs:
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#lightweight-and-debug-slice-variants"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19"
---

# Slice Lightweight Maintenance And Handoff

## Overview

This skill closes the lightweight TDD path without pretending that local proof, a result, or a promotion check automatically leaves the Slice healthy. The core rule is: record the smallest checkpoint that preserves truth, ownership, and next action, then write a handoff only when someone or a later Slice needs it.

It is for the optional final pause point in `slice.lightweight-tdd-development`. It is a checkpoint and routing skill: it prepares `review.md` and/or `handoff.md`, chooses the honest local terminal route, and stops. It does not execute maintenance repairs, mutate source or indexes, promote durable knowledge, deploy, inspect live systems, or expand the Slice into a full workflow.

## When to Use

Use when the lightweight Slice has local verification and result context, or when it is paused, blocked, superseded, transferred, or waiting on follow-up work. Use it after `slice-lightweight-result-writer` or `slice-lightweight-promotion-router` when the next honest action is to preserve cleanup status, deferred work, owner, review need, stale artifacts, or resume context.

Use it when `handoff.md` is needed for a user, teammate, future agent, blocked dependency, missing authority, pending deploy/live proof, stale branch, uncommitted work, or follow-up Slice. Use it when `review.md` is needed before closure.

Trigger boundary: select this skill only when the active question is "what checkpoint, handoff, or local terminal route preserves the Slice truth now?" Non-trigger boundary: do not select it when the active question is how to choose tests, perform the TDD cycle, patch source, verify implementation, assess deploy impact, write the result, promote knowledge, repair artifacts, rebuild generated state, execute operations, modify branches, or inspect a live system. Route to debug, hybrid, operations, public maintenance, result, promotion, verification, or user clarification when the next action is outside lightweight closure.

## Source Contract

- Manifest step: `capabilities/pipelines/slice-variants/lightweight-tdd.pipeline.json#step_graph.steps.slice-lightweight-maintenance-and-handoff`.
- Architecture anchors: `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#lightweight-and-debug-slice-variants`, `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6`, and `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19`.
- Atom anchor: `capabilities/pipelines/slice-variants/lightweight-tdd.pipeline.json#step_graph.steps.slice-lightweight-maintenance-and-handoff.invokes.slice-lightweight-maintenance-and-handoff`.

The step is optional, invokes this skill, produces `handoff.md` and `review.md`, gates on `maintenance_checkpoint_recorded`, and exits as `completed_local_verified`, `handoff_required`, or `superseded_by_followup_slice`. Lightweight closure is low ceremony, not low proof.

Required source inputs are the active Slice contract/front door, variant-selection record, `implementation-notes.md`, `verification.md`, `result.md` when it exists, promotion or no-promotion decision, `deferred.md` if any work remains, deploy-impact status, workspace/git preflight notes, changed-path list, residual risks, authority blockers, cleanup status, owner of next action, and any existing `handoff.md` or `review.md`. If one of these inputs is absent, record the absence as a checkpoint fact; do not invent it from chat memory.

## Operating Procedure

1. Load the Slice contract, result, verification evidence, promotion or deferred record, deploy-impact status, workspace/git preflight notes, changed-file list, residual risks, and any existing handoff. If inputs are missing, record the gap before choosing closure.
2. Classify the closure posture: locally verified and no handoff needed, handoff required, blocked by missing proof or authority, superseded by a follow-up Slice, or routed to the public maintenance surface for repair proposal. Keep this classification tied to the proof already recorded; do not upgrade local proof into live or durable truth.
3. Check minimal maintenance facts. Confirm result presence, proof freshness, deferred items, promotion decision, cleanup or branch state, next-action owner, stale or untracked artifacts, and whether optional evidence or review notes have a lifecycle. This is a checkpoint, not a repair run.
4. Choose the route before writing output: local close, handoff to user/team/future agent, follow-up Slice, public maintenance request, debug/root-cause escalation, hybrid/ops escalation, or block for missing source truth. The route must name the owner of next action and the reason this skill is not performing that action.
5. Write `review.md` when a checkpoint is needed. Include Slice id, checked artifacts, maintenance verdict, stale or missing items, cleanup recommendation, owner, terminal state, proof gate status, and why no broader maintenance action is being taken.
6. Write `handoff.md` when continuation is needed. Include highest validated truth, changed paths, proof already run, proof still missing, open risks, blocked authority, exact next action, and what must not be claimed after resume.
7. Choose one terminal state. Use `completed_local_verified` only when the Slice has fresh local proof, result context, no active handoff need, and no unresolved lightweight maintenance checkpoint. Use `handoff_required` when another actor or future session must continue. Use `superseded_by_followup_slice` when a new Slice owns the remaining work.
8. Stop after recording the checkpoint and handoff. Do not create repair patches, branch changes, index/front-door updates, result truth, promotion writes, durable knowledge, deployments, or live validation from this skill; route those to their owning surface or variant.

## Outputs

Produce `review.md` when the Slice needs a checkpoint before closure. It must name checked artifacts, maintenance verdict, result/proof status, deferred or promotion state, cleanup status, stale or missing items, owner, terminal state, and next route.

Produce `handoff.md` when continuation is required. It must preserve highest validated truth, completed proof, unavailable or pending proof, changed paths, residual risk, authority blockers, exact next action, forbidden claims, and resume context. Valid terminal states are `completed_local_verified`, `handoff_required`, and `superseded_by_followup_slice`.

If no file is written, the skill output must still state why `review.md` and `handoff.md` are unnecessary, what source inputs were checked, which proof gate is satisfied, and which terminal route was selected. A no-file path is allowed only when the existing Slice artifacts already preserve the same facts from a cold resume.

## Verification

Verify that `maintenance_checkpoint_recorded` is satisfied before leaving this step. The checkpoint must separate local proof from deploy/live truth, identify whether result and promotion/deferred decisions exist, state cleanup status, name the owner of next action, list forbidden claims, and say whether stale artifacts, branch/worktree cleanup, or follow-up work remain.

If `handoff.md` exists, verify it is actionable from a cold resume: the next actor can identify the Slice, truth, changed files, proof state, blockers, and next command or decision without chat memory. If no handoff is written, verify the reason is explicit.

For skill-body validation, run `node tools/validate-internal-skill-body-quality.mjs --skill slice-lightweight-maintenance-and-handoff` and `node tools/validate-internal-skill-trigger-fixtures.mjs --skill slice-lightweight-maintenance-and-handoff`.

## Failure Modes

Use `handoff_required` when proof is incomplete, authority is missing, deploy or live validation is pending, user or team continuation is needed, branch/worktree cleanup is unsafe, artifacts are stale, or the next action cannot be completed in the current session.

Use `superseded_by_followup_slice` when the remaining work is larger than lightweight closure, belongs to debug, hybrid, operational, research, durable knowledge, or public maintenance flow, or needs a fresh Slice with its own proof contract.

Block instead of closing when result context is absent, the checkpoint cannot distinguish local from live truth, required sources are unavailable, ownership is unclear, or writing a handoff would hide unresolved work. Do not silently repair, promote, mutate, deploy, update indexes/front doors, write result truth, promote durable knowledge, or claim final completion from this step.

The checkpoint should leave zero ambiguity about whether the Slice is locally closed, waiting on another actor, or superseded by follow-up work.
