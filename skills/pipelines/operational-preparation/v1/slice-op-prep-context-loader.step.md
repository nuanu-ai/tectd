---
id: "slice-op-prep-context-loader"
kind: "internal_step"
owner_family: "slice_variant"
pipeline_id: "slice.operational-preparation"
step_id: "slice-op-prep-context-loader"
entry_gate: false
public_trigger: false
native_skill_discovery: false
body_path: "capabilities/instructions/pipelines/slice-variants/operational-preparation/slice-op-prep-context-loader.step.md"
source_manifest: "capabilities/pipelines/slice-variants/operational-preparation.pipeline.json"
legacy_skill_ref: "slice-op-prep-context-loader"
architecture_refs:
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#operational-preparation-and-operational-execution-variants"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19"
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#operational-and-hybrid-slice-variants"
---

# Operational Preparation Context Loader

## Overview
This skill loads source context to prepare a future operation for `slice.operational-preparation`. Its core rule is prep-only authority: build an honest context packet from available evidence, declare missing context, and stop before any command execution or target mutation.

The loader prepares later operational-preparation steps. It does not baseline live state, build command plans, run dry-runs, execute operations, verify post-action proof, or write result truth.

## When to Use
Use this after operational intent is captured and before the preparation contract or target-state baseline is written. Select it when a Slice needs operation intent, target state, environment map, runbook/procedure refs, proof needs, authority boundaries, user/team handoff constraints, risk or rollback clues, source freshness, and missing-context blockers before preparing commands or handoff.

Do not use it when the agent already has explicit bounded authority to mutate a target, when code or config changes are part of the work, when root cause is unknown and debug is needed first, or when the operation has already been performed and needs execution proof instead of preparation context. It routes execution, hybrid implementation, or debug escalation when prep no longer fits.

## Source Contract
This skill is grounded in `capabilities/pipelines/slice-variants/operational-preparation.pipeline.json`, step `slice-op-prep-context-loader`. The manifest marks the step required, invokes `skill:slice-op-prep-context-loader`, produces `README.md`, gates on `context_loaded_or_declared_missing`, fails by `request_context_or_block`, and reaches `context_ready`.

Architecture anchors are `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#operational-preparation-and-operational-execution-variants`, `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#operational-and-hybrid-slice-variants`, `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6`, `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19`, and `docs/architecture/master-plugin-target-architecture-capability-atom-shard-map.html`. The atom row for `pipeline.slice.operational_preparation.context.loader` defines the job as loading deployment docs, runbooks, environment maps, service state, prior incidents, and parent constraints.

## Source Inputs
Load only evidence already available to the agent through user-provided context, local source files, existing docs, previous Slice/Scope artifacts, or already captured logs/status notes. Useful inputs include:

- operation intent, operation target, desired final state, non-goals, and target environment;
- parent Program/Epoch/Scope/Slice constraints, current artifact contract, and prior transition or handoff notes;
- deployment docs, runbooks, procedure refs, service/env maps, config ownership notes, and credential-location notes without secret values;
- already captured service state, version/config notes, incident records, rollback notes, and proof examples;
- user/team authority, approval, timing, maintenance window, ownership, and handoff constraints.

This skill must not run commands, mutate systems, or create execution/result claims. If context freshness requires a command, live probe, dry-run, API call, DB query, deploy check, log query, or repo/worktree inspection, record that need as missing context and route it to a later authorized read-only, dry-run, baseline, execution, or handoff step.

## Operating Procedure
1. Confirm the selected packet is an operational-preparation Slice and that operation intent exists. If target, desired final state, or authority boundary is absent, stop at `request_context_or_block`.
2. Build the context inventory from already available source inputs. Keep each item tied to its source path, user note, prior artifact, or captured evidence pointer.
3. Classify every input as `loaded`, `declared_missing`, `stale`, `freshness_unknown`, `sensitive`, `conflicting`, `outside_authority`, or `not_needed_for_prep`. Do not smooth over conflicts or stale source truth.
4. Extract the prep facts later steps need: target system, target state, environment, executor candidate, approval boundary, runbook/procedure refs, proof needs, handoff recipient, rollback clues, blast-radius clues, stop-condition hints, and known constraints.
5. Mark missing context blockers separately from ordinary gaps. A blocker includes missing target, final state, authority boundary, essential runbook/procedure location, environment identity, executor owner, or proof requirement.
6. Preserve source freshness. Label dates, versions, captured-at timestamps, unknown freshness, and which later step must refresh or verify the source before action.
7. Write the `README.md` context section as a preparation packet: loaded sources, declared gaps, authority/freshness/proof caveats, risk and rollback clues, user/team handoff constraints, and next-step routing for each unresolved item.
8. End with `context_ready` only when later preparation steps can proceed honestly from loaded or explicitly missing context. End with `request_context_or_block` when essential context is absent or only discoverable through unauthorized command execution.

## Outputs
The output is the context portion of `README.md` for the operational-preparation package. It must include:

- operation target and desired final state;
- authority boundary and executor/handoff constraints;
- environment map and runbook/procedure refs;
- loaded source list with source pointers and freshness labels;
- missing, stale, sensitive, conflicting, or outside-authority context;
- proof needs, risk/rollback clues, and source-refresh needs;
- next-step routing to contract writer, authority boundary, target-state baseliner, risk modeler, preflight builder, command-plan builder, proof builder, user handoff, execution escalation, hybrid escalation, or debug escalation.

The terminal state is either `context_ready` or `request_context_or_block`. This skill does not produce command plans, rollback plans, proof contracts, handoff packages, promotion records, or operation-completed results; later operational-preparation steps own those artifacts.

## Verification
Verify that the selected manifest is `capabilities/pipelines/slice-variants/operational-preparation.pipeline.json` and the active step is `slice-op-prep-context-loader`. Check that `README.md` context names the operation target, desired final state, authority boundary, parent constraints, loaded source classes, and any missing or stale sources.

Also verify negative boundaries: no command was run by this step, no target state was mutated, no deploy/write/delete/seed/migrate action was performed, no hidden secret was copied into the artifact, and no completed-operation claim appears. This skill forbids target mutation and completed-operation claims. Any need for execution authority, implementation, or debug must be routed as an escalation rather than hidden inside context prose.

## Failure Modes
Block or request context when operation target, desired final state, authority boundary, target environment, executor owner, proof need, or essential source location is missing. Block when the only way to learn required context is command execution, unauthorized live-system access, credential disclosure, repo/worktree mutation, or a deployment-like action.

Escalate instead of continuing when explicit execution authority is granted now, code or configuration changes are needed, unknown root cause dominates, incident response takes over, or loaded context contradicts the selected operational-preparation variant. Preserve stale, conflicting, sensitive, and unavailable sources in the output so later steps can decide whether to ask the user, run allowed read-only validation, or hand off.

Use `request_context_or_block` rather than guessing when source freshness cannot be established, authority is ambiguous, the user/team handoff owner is unknown, rollback clues are absent for a high-risk operation, or proof needs are unclear enough that later operation claims would be unsafe.
