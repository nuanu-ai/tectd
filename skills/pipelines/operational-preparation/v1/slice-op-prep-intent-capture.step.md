---
id: "slice-op-prep-intent-capture"
kind: "internal_step"
owner_family: "slice_variant"
pipeline_id: "slice.operational-preparation"
step_id: "slice-op-prep-intent-capture"
entry_gate: false
public_trigger: false
native_skill_discovery: false
body_path: "capabilities/instructions/pipelines/slice-variants/operational-preparation/slice-op-prep-intent-capture.step.md"
source_manifest: "capabilities/pipelines/slice-variants/operational-preparation.pipeline.json"
legacy_skill_ref: "slice-op-prep-intent-capture"
architecture_refs:
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#operational-preparation-and-operational-execution-variants"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19"
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#operational-and-hybrid-slice-variants"
---

# Operation Intent Capture

## Overview

This skill records the intent for a preparation-only operation before any context loading, authority declaration, risk modeling, command planning, preflight, rollback, proof-contract, result, promotion, or handoff work starts. It owns only `operation-intent.md`: what operation is wanted, what success means, what is out of scope, who may do what, and what is still missing. It must not execute, mutate, baseline live state, or claim readiness beyond `intent_recorded`.

## When to Use

Use this skill after the operational-preparation entry gate selects `slice.operational-preparation` and before downstream prep steps need `operation-intent.md`. It fits requests for deploy, redeploy, seed, rollback, data repair, service restart, infrastructure change, chain/API operation, or recovery planning when the user wants a safe package but has not authorized execution.

Do not use it when the target operation has already been executed, when the user grants immediate execute/deploy/write authority, when root cause is unknown and debug must come first, or when the work is implementation rather than preparation. Do not use it to gather live state, write command plans, run dry-runs, baseline target state, promote a runbook, or claim `prepared_not_executed` result closure; later operational-preparation steps own those outputs.

## Source Contract

- Architecture: `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#operational-preparation-and-operational-execution-variants` defines preparation as an exact safe operation package without target mutation, and `#operational-and-hybrid-slice-variants` defines escalation to operational execution or hybrid when authority or implementation changes the shape. `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6` defines Slice/result proof boundaries, and `#s19` selects operational prep when the signal is an ops target with no execution.
- Manifest: `capabilities/pipelines/slice-variants/operational-preparation.pipeline.json` step `slice-op-prep-intent-capture` produces `operation-intent.md`, gates on `target_and_final_state_captured`, fails with `block_missing_intent`, and reaches `intent_recorded`.
- Atom/manifest anchors: `pipeline.slice.operational-preparation` and `step_graph.steps.slice-op-prep-intent-capture` ground this as the intent-capture step for the operational-preparation variant.
- External references: no external skill body is referenced for this exact step.

## Source Inputs

Read the active runtime or Slice context before writing. Required inputs are the user request, entry-gate decision that selected `slice.operational-preparation`, parent Slice target if one exists, any supplied target/environment names, any supplied desired final state, any stated non-goals, and any stated authority limits. Treat absent inputs as explicit unknowns to ask about; do not infer production targets, credentials, execution permission, or success proof from convention.

## Operating Procedure

1. Confirm the active slice is preparation-only: the selected variant is `slice.operational-preparation`, execution authority is absent or explicitly withheld, and the next output needed is `operation-intent.md`.
2. Run the target sweep. Ask: What exact system, repo, service, environment, chain, database, account, host, artifact, or bounded object is the operation about? If the target could mean more than one thing, ask one missing-field question instead of guessing.
3. Run the desired-state sweep. Ask: What observable final state should exist after a future executor acts? Prefer version, status, config, balance, record state, endpoint behavior, deployment posture, queue posture, or another proofable state.
4. Run the non-goal sweep. Ask: What systems, data, commands, environments, user-visible behaviors, or adjacent fixes are explicitly out of scope or forbidden?
5. Run the authority sweep. Ask: Is this read-only, prep-only, user-executes-manually, agent-may-dry-run-later, blocked, or requires fresh approval before execution? Separate manual responsibilities from agent responsibilities.
6. Run the constraint sweep. Ask: What timing window, downtime tolerance, credentials boundary, protected branch or production caution, required cwd/environment, allowed tools, communication expectation, or compliance constraint applies?
7. Run the success-evidence sweep. Ask: What evidence class should later steps prepare for: logs, status endpoint, DB query, version/ref, health check, UI smoke, chain event, user confirmation, or another proof class? Name the class only; do not collect proof here unless the user already supplied it.
8. If target, final state, non-goals, authority posture, user constraints, success evidence, manual responsibilities, or agent responsibilities are missing, write concise missing-field questions and stop with `block_missing_intent`. End in `intent_recorded` only when `target_and_final_state_captured` is satisfied.

## Outputs

Write only `operation-intent.md`. Use this shape:

- Operation summary
- Target and environment
- Desired final state
- Non-goals and forbidden outcomes
- Authority posture
- User constraints
- Success evidence to prepare for
- Manual responsibilities
- Agent responsibilities
- Missing-field questions
- Gate verdict: `target_and_final_state_captured` or `block_missing_intent`
- Terminal state: `intent_recorded` or `block_missing_intent`

Allowed terminal states are `intent_recorded` and `block_missing_intent`. The output must preserve preparation truth: no mutating command execution, no deploy/write/delete/seed/migrate action, no hidden credential capture, and no operation-completed claim.

When `intent_recorded`, route the next preparation step to `slice-op-prep-context-loader`. When blocked, return only the missing-field questions to the user or parent runtime. When the user grants execution authority, route to `slice.operational-execution`; when implementation plus deploy/live proof is required, route to `slice.hybrid-implementation-operation`; when root cause is unknown, route to debug/root-cause before operation prep continues.

## Verification

Validate this body with `node tools/validate-internal-skill-body-quality.mjs --skill slice-op-prep-intent-capture` and trigger coverage with `node tools/validate-internal-skill-trigger-fixtures.mjs --skill slice-op-prep-intent-capture`. For actual use, verify `operation-intent.md` answers: what target, what desired final state, what non-goals, what authority posture, what constraints, what success evidence, who executes manually, what the agent may prepare, and what questions remain.

Also scan the intent record for forbidden overreach: no command list, no baseline evidence collection, no dry-run result, no rollback plan, no proof result, no runbook promotion, no target mutation, and no claim that the operation is complete or even ready beyond `intent_recorded`.

## Failure Modes

Stop with `block_missing_intent` when the target is ambiguous, the desired final state is not observable, non-goals are absent for a risky target, authority is unclear, user constraints conflict, responsibilities are mixed, or success evidence cannot be named. Ask the smallest set of missing-field questions needed to unblock the next preparation step.

Route away instead of stretching this skill: debug if unknown failure cause dominates, operational execution if explicit execution authority exists now, hybrid if implementation plus deploy/live proof is required, procedure capture if the user is preserving a reusable workflow, or maintenance/query if the request is only to inspect stale artifacts or current state.

Treat zero execution authority as a blocker to record, not a reason to infer permission or continue into later operational steps.
