---
id: "slice-op-exec-command-ledger-builder"
kind: "internal_step"
owner_family: "slice_variant"
pipeline_id: "slice.operational-execution"
step_id: "slice-op-exec-command-ledger-builder"
entry_gate: false
public_trigger: false
native_skill_discovery: false
body_path: "capabilities/instructions/pipelines/slice-variants/operational-execution/slice-op-exec-command-ledger-builder.step.md"
source_manifest: "capabilities/pipelines/slice-variants/operational-execution.pipeline.json"
legacy_skill_ref: "slice-op-exec-command-ledger-builder"
architecture_refs:
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#operational-preparation-and-operational-execution-variants"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19"
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#operational-and-hybrid-slice-variants"
---

# Operational Execution Action Ledger Builder

## Overview
This skill turns an approved operational execution intent into `action-ledger.md`, the ordered action record used by the later action runner. It models exact commands and tool calls before mutation, prevents unlogged or context-drifted operations, and never runs actions or grants execution authority.

## When to Use
Use this after `authority-confirmation.md`, `current-state.md`, `preflight.md`, `risk-stop-conditions.md`, and final approval exist for a selected `slice.operational-execution` path. It applies when the agent must prepare the exact action sequence for a bounded target environment, including actor, authority source, command order, target identity, checkpoints, expected output, proof path, timeout, stop conditions, and rollback or recovery links.

Do not use it for preparation-only handoffs, unknown-root-cause work, implementation work, incident workflows that supersede the slice, missing or ambiguous authority, rollback execution, post-action validation, or completion claims. Do not use it after `action-ledger.md` has already been consumed by execution unless a new approved manifest pass reopens the ledger.

## Source Contract
Ground this skill in `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#operational-preparation-and-operational-execution-variants`, `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#operational-and-hybrid-slice-variants`, `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6`, and `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19`.

The owning manifest is `capabilities/pipelines/slice-variants/operational-execution.pipeline.json`. The exact step is `slice-op-exec-command-ledger-builder`, under `pipeline.slice.operational-execution`, producing `action-ledger.md`, gating on `action_sequence_recorded`, failing by `stop_or_handoff`, and reaching `ready_for_next_step` only when the ledger can drive the next step without interpretation. The artifact contract also requires an `action-ledger-record` and later proof boundaries; this skill only prepares those inputs.

## Operating Procedure
1. Confirm the selected variant is `slice.operational-execution` and that prior artifacts record explicit authority, current state, preflight status, risk-stop conditions, and final approval. If any are missing, stale, or contradictory, stop with `stop_or_handoff`.
2. Lock target identity from the slice artifacts: workspace, repo, host, target environment, service, branch, commit, account, chain, database, endpoint, or other bounded target. Every ledger row must reference that identity or an exact subtarget.
3. Identify the actor for each row: agent, user, named teammate, automated tool, or blocked/manual owner. The actor must match the authority source and must not be upgraded by this builder.
4. Collect only approved commands or tool calls from the approved operation plan, runbook, handoff, or final approval. Convert prose into a row only when the command or tool call can be written byte-for-byte with exact arguments; otherwise block instead of guessing.
5. Classify each row as read-only, dry-run, mutating action, checkpoint, rollback, or recovery handoff. Read-only and dry-run rows still need target identity, cwd, expected output, proof path, timeout, and stop conditions; they are not permission to mutate.
6. Record each row with sequence ID, action type, actor, authority source, target identity, exact command or tool call plus arguments, cwd, env variable names or secret source references without secret values, timeout, preconditions, expected output, expected proof, proof path, checkpoint rule, stop conditions, rollback or recovery link, retry policy, and next owner.
7. Keep order explicit. Separate checkpoints from actions so the action runner can stop after one row and the checkpoint verifier can prove state before any following mutation. Use "no hidden retries" as the retry policy unless the approval artifact names a bounded retry.
8. Validate every row against authority and risk-stop artifacts. Reject rows that exceed authority, change target identity, lack a timeout or proof expectation, skip preflight, continue after a stop condition, imply rollback without rollback authority, or introduce an unlogged command.
9. Check context drift immediately before readiness: compare the frozen target, actor, authority source, cwd, target environment, branch/commit/service version, and proof path against current-state and preflight artifacts. If any value drifted, return `stop_or_handoff`.
10. Write `action-ledger.md` with a prerequisite summary, frozen target table, ordered ledger table, checkpoint map, proof expectations, timeout policy, rollback or recovery map, secret-handling note, open gaps, and terminal decision. Use `ready_for_next_step` only when each action can be attempted exactly as written; otherwise use `stop_or_handoff`.

## Outputs
The required output is `action-ledger.md` in the selected Slice folder under the operative spine. It must contain enough detail for a later worker to attempt one action at a time without inventing command text, target scope, timeout, proof, or recovery behavior. The ledger should also support the required `action-ledger-record` by naming source artifacts, authority references, target identity, row count, blocked rows, and the next manifest owner.

Each row must be auditable back to authority and proof: actor, authority source, target environment, exact command/tool shape, cwd, env names or secret source reference, expected output, proof path, checkpoint, stop condition, and recovery relation. Secret values must stay out of the ledger.

Do not write `execution-log.md`, `post-action-validation.md`, `result.md`, or `rollback.md` from this skill. Do not execute a command from this skill unless a future manifest explicitly changes this step's responsibility.

## Verification
Verify the body against `tools/validate-internal-skill-body-quality.mjs --skill slice-op-exec-command-ledger-builder` and the trigger fixture against `tools/validate-internal-skill-trigger-fixtures.mjs --skill slice-op-exec-command-ledger-builder`.

Content verification checks that the skill references the required architecture and manifest sources, has exactly the seven Layer 6B H2 sections, and contains concrete ledger behavior rather than wrapper boilerplate. Ledger verification checks for exact command or tool-call rows, actor, authority source, target identity, cwd, target environment, env handling, expected output, expected proof, proof path, timeout, checkpoint rule, rollback or recovery link, stop conditions, no hidden retries, secret-value exclusion, dry-run/read-only distinction, and context-drift prevention.

## Failure Modes
Stop or hand off when execution authority is absent, final approval is missing, target identity cannot be locked, preflight is failed or stale, risk-stop conditions are incomplete, command text is ambiguous, a tool call lacks arguments, a row needs secret values in the ledger, proof cannot be named, timeout is missing, rollback would be needed without rollback authority, or incident scope supersedes the slice.

Also stop if the requested action would mutate a different target, add an unapproved retry, skip a checkpoint, continue after a stop condition, run an unlogged command, blur read-only and mutating actions, or claim the operation is complete before later execution and post-action proof exist.

If a zero-proof completion claim appears, reject it and route to the later execution, checkpoint, post-action validation, result, or recovery owner.
