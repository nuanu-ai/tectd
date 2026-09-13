---
id: "slice-op-exec-context-loader"
kind: "internal_step"
owner_family: "slice_variant"
pipeline_id: "slice.operational-execution"
step_id: "slice-op-exec-context-loader"
entry_gate: false
public_trigger: false
native_skill_discovery: false
body_path: "capabilities/instructions/pipelines/slice-variants/operational-execution/slice-op-exec-context-loader.step.md"
source_manifest: "capabilities/pipelines/slice-variants/operational-execution.pipeline.json"
legacy_skill_ref: "slice-op-exec-context-loader"
architecture_refs:
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#operational-preparation-and-operational-execution-variants"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19"
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#operational-and-hybrid-slice-variants"
---

# Operational Execution Context Loader

## Overview

This skill performs operational execution context intake for `slice.operational-execution`. Its core rule is: load the smallest current operational packet that lets later steps decide authority, preflight, command planning, risk, and proof without guessing.

The loader is not an executor. It does not run commands, does not mutate systems, does not deploy, does not perform rollback or recovery, and does not create result claims.

## When to Use

Use after `slice-op-exec-entry-gate` accepts `slice.operational-execution` and before authority confirmation, current-state baselining, preflight, or command ledger work needs to continue. Select it when the active Slice has a bounded operation target and needs the current operation plan, runbook, prepared handoff, environment map, repo state, credential constraints, proof needs, stop-condition notes, or parent Scope constraints loaded into a reconstructable packet.

Do not use it for preparation-only work, current-state baselining, authority confirmation, preflight execution, command ledger construction, action running, rollback/recovery execution, post-action validation, result writing, procedure capture, durable runbook mutation, or live incident ownership. Route those to their own operational execution, operational preparation, hybrid, debug, procedure, result, or maintenance steps.

## Source Contract

This skill implements `capabilities/pipelines/slice-variants/operational-execution.pipeline.json#step_graph.steps.slice-op-exec-context-loader`. The step is required, invokes `skill:slice-op-exec-context-loader`, produces `README.md`, gates on `context_loaded_or_declared_missing`, fails by `stop_or_handoff`, and advances only as `ready_for_next_step`.

Architecture source truth is `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#operational-preparation-and-operational-execution-variants`, `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#operational-and-hybrid-slice-variants`, `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6`, and `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19`. The atom row is `pipeline.slice.operational_execution.context.loader` in `docs/architecture/master-plugin-target-architecture-capability-atom-shard-map.html`, summarized as loading operation plan, runbook, handoff, env map, state, repo state, credentials constraints, and proof needs.

The operational execution manifest gives later owners authority confirmation, current-state capture, preflight, risk-stop checks, final approval, command ledger construction, action execution, rollback/recovery, observation, result writing, and promotion routing. This loader can point to those next owners, but it must not perform post-action validation or any other later-step work.

## Operating Procedure

1. Confirm variant and target identity. The active packet must name `slice.operational-execution`, parent Slice or Scope, target system, environment, desired final state, entry-gate verdict, and why execution may be considered now. If target identity or parent ownership is missing, stop with `stop_or_handoff`.
2. Load source inputs in priority order: current Slice `README.md` and `slice.md`, operation plan, prepared handoff, runbook, source-operation plan, environment map, current state or declared unknown, prior dry-run or read-only proof, repo state, command plan references, risk/stop conditions, rollback/recovery hints, proof contract, user instructions, and ownership notes.
3. Build the minimal operational packet. Required fields are target identity, action contract, authority posture, current state source, dry-run/read-only proof status, environment/source references, repo state source, credential constraints, command plan references, risk/stop conditions, rollback/recovery hints, proof needs, ownership, and missing context blockers.
4. Classify each context item as loaded, missing, stale, contradictory, restricted, sensitive, derived, historical, or unnecessary. Mark freshness explicitly; historical plans, generated projections, old handoffs, and memory-derived notes are leads until a later current-state or preflight step refreshes them.
5. Apply the authority/freshness/proof gates. Do not mark context ready if authority posture is absent, the action contract is unclear, proof needs are unnamed, required read-only or dry-run proof is missing without consequence, stop conditions are unavailable, rollback/recovery expectations are missing for risky work, or ownership of the next action is unknown.
6. Apply secrets handling. Record credential needs, secret locations, required account or role, and access blockers as constraints only. Never copy tokens, passwords, keys, raw connection strings, private dashboard URLs that reveal secrets, or credential values into `README.md`.
7. Separate next-step routing. If context is sufficient, route to `slice-op-exec-authority-confirmation` or the next manifest step named by Runtime. If the problem is fresh state, route to `slice-op-exec-current-state-baseliner`; if it is preflight, route to `slice-op-exec-preflight-runner`; if it is command sequencing, route to `slice-op-exec-command-ledger-builder`; if it is missing prep, route to operational preparation or handoff.
8. Write the `README.md` context block. Use compact bullets or a table with source path/reference, source class, freshness, authority sensitivity, proof relationship, loaded/missing status, consequence, and next owner.
9. Set `context_loaded_or_declared_missing` only when every required item in the operational packet is loaded or explicitly declared missing with consequence. End as `ready_for_next_step` only when the next step can act on the packet without inferring hidden facts; otherwise end as `stop_or_handoff`.

## Outputs

Produce only the operational execution context section of `README.md`. The output shape is:

- `Variant and target`: Slice id, target identity, environment, desired final state, action contract, and parent ownership.
- `Loaded sources`: operation plan, runbook, prepared handoff, environment map, current state reference, repo state reference, dry-run/read-only proof, command plan references, risk/stop conditions, rollback/recovery hints, and proof needs.
- `Authority and secrets`: authority posture, credential constraints, secrets handling note, restricted sources, and actor or owner responsible for resolving access.
- `Freshness and proof`: source class, timestamp or freshness label, proof relationship, stale/contradictory items, and proof gaps.
- `Missing context blockers`: missing item, consequence, owner, and whether the next route is ask user, refresh, handoff, operational preparation, current-state baseline, preflight, or block.
- `Terminal states`: `ready_for_next_step` when the packet is usable, or `stop_or_handoff` when missing context blocks safe continuation.

This skill does not create `authority-confirmation.md`, `current-state.md`, `preflight.md`, `risk-stop-conditions.md`, `action-ledger.md`, `execution-log.md`, `post-action-validation.md`, `result.md`, or `recovery-notes.md`. It may reference those artifacts as downstream owners.

## Verification

Verify the trigger fit: positive cases have an accepted operational execution entry gate and need minimal current context before authority confirmation, current-state baseline, preflight, or command planning. Negative cases must not select this skill when the work is preparation-only, current-state capture, authority confirmation, preflight, action running, rollback/recovery, post-action validation, result writing, procedure capture, durable runbook mutation, or incident response.

Run `node tools/validate-internal-skill-body-quality.mjs --skill slice-op-exec-context-loader` and `node tools/validate-internal-skill-trigger-fixtures.mjs --skill slice-op-exec-context-loader`. Also parse both fixture JSON files, scan this body for the seven required H2 sections in order, run scoped whitespace and final-newline checks on the three owned files, and run `git diff --check -- skills/slice-op-exec-context-loader/SKILL.md validation/fixtures/internal-skill-body-quality/slice-op-exec-context-loader.json validation/fixtures/internal-skill-trigger/slice-op-exec-context-loader.json`.

Content verification must confirm the `README.md` packet names target identity, action contract, authority posture, source freshness, dry-run/read-only proof status, command plan references, risk/stop conditions, rollback/recovery hints, proof needs, ownership, secrets handling, missing context blockers, terminal states, and handoff routing. It must also confirm there is no command execution, system mutation, deployment, rollback execution, post-action proof, durable promotion, hidden credential value, or operation-completed claim.

## Failure Modes

Use `stop_or_handoff` when the variant is not selected, target identity is missing, the action contract is unclear, authority posture is absent, ownership is unknown, essential source locations are missing, context is stale beyond accepted risk, sources conflict, proof needs cannot be named, dry-run/read-only proof is required but absent, or secrets handling would expose credential values.

Block rather than normalize when preflight is already known to fail, a stop condition is already visible, rollback/recovery hints are missing for risky work, incident ownership overrides generic operation, the prepared handoff is stale, or later steps would have to infer operational facts from memory or old plans.

Route missing preparation to `slice.operational-preparation`, source-plus-live work to the hybrid variant, unknown-cause failures to debug, current-state evidence gaps to `slice-op-exec-current-state-baseliner`, command ordering gaps to `slice-op-exec-command-ledger-builder`, post-action truth to validation/result steps, and reusable process capture to procedure capture. Keep the missing item and consequence visible in `README.md`; do not downgrade proof, invent context, or treat context loading as permission to execute.
