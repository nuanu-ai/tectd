---
id: "slice-op-exec-current-state-baseliner"
kind: "internal_step"
owner_family: "slice_variant"
pipeline_id: "slice.operational-execution"
step_id: "slice-op-exec-current-state-baseliner"
entry_gate: false
public_trigger: false
native_skill_discovery: false
body_path: "capabilities/instructions/pipelines/slice-variants/operational-execution/slice-op-exec-current-state-baseliner.step.md"
source_manifest: "capabilities/pipelines/slice-variants/operational-execution.pipeline.json"
legacy_skill_ref: "slice-op-exec-current-state-baseliner"
architecture_refs:
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#operational-preparation-and-operational-execution-variants"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19"
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#operational-and-hybrid-slice-variants"
---

# Operational Execution Current State Baseliner

## Overview
Establish pre-action truth for an authorized operational execution Slice before any preflight that could mutate state, command ledger execution, deployment, restart, repair, migration, data action, or live-system operation. The core rule is: baseline fresh, source-backed current evidence or declare the gap; memory is not current state, and stale, historical, user-stated, or inferred evidence is never enough.

This is a `skill_body` implementation. It creates the comparison baseline that later proof uses to decide what changed, what was already broken, and whether a destructive or irreversible action is still safe to attempt.

## When to Use
Use this after `slice-op-exec-authority-confirmation` when `slice.operational-execution` has explicit authority for a bounded target and the next gate is `current_state_captured_before_action`. It fits production or staging deploys, migrations, restarts, data repairs, chain/API operations, service changes, account or cluster actions, queue/job intervention, and other authorized operations where later proof needs a before-state.

Do not use it for preparation-only packaging, target-state baselining without current execution authority, context loading, authority confirmation, preflight running, command ledger construction, action execution, rollback, post-action validation, result writing, or root-cause debugging. Route those cases to the matching operational preparation, operational execution, hybrid, or debug step. If the user asks for a current-state answer without an active operational execution Slice, route to query or maintenance instead of forcing this step.

## Source Contract
This skill implements `capabilities/pipelines/slice-variants/operational-execution.pipeline.json` step `slice-op-exec-current-state-baseliner`: required, invokes `skill:slice-op-exec-current-state-baseliner`, produces `current-state.md`, gates on `current_state_captured_before_action`, fails by `stop_or_handoff`, and advances to `ready_for_next_step`.

Source inputs are `authority-confirmation.md`, `operation-intent.md`, the approved operation plan, prepared handoff or runbook if present, proof or rollback expectations, and any declared target state. Grounding sources are `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#operational-preparation-and-operational-execution-variants`, `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#operational-and-hybrid-slice-variants`, `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6`, `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19`, `docs/architecture/master-plugin-target-architecture-capability-atom-shard-map.html#pipeline.slice.operational_execution.current.state.baseliner`, and `docs/architecture/master-plugin-target-architecture-capability-atom-shard-map.html#pipeline.slice.operational-execution`.

The atom-shard row requires current version, config, health, data, service, and runtime state before mutation. The final-map route selects operational execution only after explicit execution authority; operational preparation and hybrid routes remain separate when authority, code changes, or deploy/live proof scope changes.

## Operating Procedure
1. Confirm the active Slice is `slice.operational-execution`, the target is bounded, authority confirmation exists, and no mutating action has started. If an action already ran, mark before/after proof invalid and route to checkpoint verification, recovery notes, or result writing.
2. Normalize target identity and target state before collecting evidence: service, repo, environment, account/project/org, region/chain/cluster, dataset, user-visible surface, expected version/config/data/process state, and source of truth for each surface.
3. Set consequence-based freshness. Production, user-visible, financial, chain/API, data, deployment, safety, destructive, and rollback claims require fresh evidence with a freshness timestamp. Prior docs, generated indexes, old handoffs, dashboards screenshots, and memory are leads until refreshed.
4. Collect read-only baseline evidence only. Use status, describe, query, inspect, log-read, dashboard-read, API-read, and source-read operations that cannot mutate the target. Record no action commands here: no deploy, restart, write, seed, migrate, rollback, branch/worktree mutation, or command ledger execution.
5. For every evidence item, record claim, value, source path or endpoint, command/query/dashboard/log reference, actor/account context, evidence class, timestamp or freshness label, comparison value, and secret-safety note. Evidence classes include live, repo-source, config-source, data-source, process-source, service-source, API-source, log-source, dashboard-source, derived, historical, user-stated, contradictory, stale, and missing.
6. Capture all relevant state surfaces: current version/ref, deployed artifact, repo branch/worktree dirtiness, configuration and feature flags, database/data/migration posture, infrastructure identifiers, process and service health, API/chain state, log and dashboard signals, queue/job posture, dependency state, credentials presence without values, and existing incident or degraded-state indicators.
7. Compare evidence against approved preconditions from the operation plan, handoff, proof contract, rollback expectations, or authority confirmation. Mark each precondition `satisfied`, `failed`, `stale`, `contradictory`, or `unknown`, with the proof artifact that would let post-action validation compare before and after.
8. Build the destructive-risk baseline. Name irreversible or high-blast-radius surfaces, data-loss risk, downtime risk, security/user impact, migration/delete/restart/deploy risk, rollback availability, observation-window need, and any proof artifact that must exist before the action is safe.
9. Resolve staleness and contradictions before proceeding. Prefer the highest-authority read-only source; refresh safely when allowed; record unresolved conflicts explicitly. Do not average sources, trust the newest-looking note, or infer target state from memory.
10. Name stop and handoff conditions visible before action: ambiguous target, wrong account/environment, stale evidence that cannot be refreshed, failed precondition, degraded health, unavailable proof source, missing rollback authority, unsafe destructive-risk baseline, or evidence that would require mutation to obtain.
11. Write `current-state.md` and emit the verdict. Use `baseline_current` only when relevant surfaces are fresh enough and comparable; use `baseline_gap_declared` when gaps are known but explicitly accepted for handoff or downgrade; use `stop_or_handoff` when the gap blocks safe execution. Set `ready_for_next_step` only after the verdict supports preflight.

## Outputs
Produce `current-state.md` for the active operational execution Slice. Required shape:

- target identity, target state, operation boundary, authority reference, and actor/account/environment;
- source-of-truth map with freshness policy, freshness timestamp, and evidence class rules;
- baseline evidence table for version, config, data, process, service, API, log, dashboard, repo, dependency, queue/job, and health surfaces where relevant;
- expected preconditions with status and comparison values;
- destructive-risk baseline, risk deltas, rollback posture, proof gaps, proof artifacts, and observation needs;
- contradiction, staleness, unknown, or missing-source handling;
- stop conditions, handoff route, and verdict: `baseline_current`, `baseline_gap_declared`, or `stop_or_handoff`;
- next-step route: preflight, risk-stop check, command ledger, maintenance/handoff, debug, recovery, or result.

This output is comparison evidence for preflight, risk-stop checks, action ledger, post-action validation, result, and recovery notes. It must not mutate the target, run the operation, deploy, write source, change branches, restart services, expose secrets, or claim the operation completed.

## Verification
Verify the body with `node tools/validate-internal-skill-body-quality.mjs --skill slice-op-exec-current-state-baseliner` and trigger scenarios with `node tools/validate-internal-skill-trigger-fixtures.mjs --skill slice-op-exec-current-state-baseliner`. Confirm the skill keeps exactly the seven required H2 sections and cites the operational-execution manifest plus architecture HTML anchors.

For actual use, inspect `current-state.md`: each relevant surface must have a source, evidence class, freshness timestamp or label, precondition status, comparison value, and proof artifact, or a named gap with consequence. Verify stale evidence is not promoted to baseline, memory is not current state, no action commands were run, no secret value is exposed, and no deployment, rollback, branch/worktree mutation, post-action proof, or completion claim appears in the baseline.

## Failure Modes
Stop or hand off when authority is absent or ambiguous, the target identity or account/environment cannot be proven, current evidence is unavailable, sources conflict, a required source is stale, a precondition fails, the destructive-risk baseline is unsafe, proof artifacts are missing, rollback authority is absent, or the baseline would require mutating the target. Route to debug when the current state is an unexplained failure state rather than an operation baseline.

If an action already started before baseline capture, mark the baseline invalid for before/after proof and hand off to checkpoint verification, recovery notes, or result writing as appropriate. If the user asks to proceed despite stale, missing, or contradictory evidence, preserve the gap and consequence instead of downgrading the baseline standard.

Keep the zero-mutation baseline consequence visible in the handoff so later operational steps know whether pre-action truth was satisfied, accepted with gaps, or blocked.
