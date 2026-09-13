---
id: "slice-op-prep-maintenance-check-requester"
kind: "internal_step"
owner_family: "slice_variant"
pipeline_id: "slice.operational-preparation"
step_id: "slice-op-prep-maintenance-check-requester"
entry_gate: false
public_trigger: false
native_skill_discovery: false
body_path: "capabilities/instructions/pipelines/slice-variants/operational-preparation/slice-op-prep-maintenance-check-requester.step.md"
source_manifest: "capabilities/pipelines/slice-variants/operational-preparation.pipeline.json"
legacy_skill_ref: "slice-op-prep-maintenance-check-requester"
architecture_refs:
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#operational-and-hybrid-slice-variants"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19"
---

# Operational Preparation Maintenance Check Requester

## Overview
This skill assembles the maintenance-check request for an operational-preparation Slice. Its core rule is request and assess readiness signals only: stale indexes, variant shape, result presence, projection freshness, promotion readiness, and repair proposal needs belong to the maintenance layer, not to this skill as a repair executor. Maintenance findings are classified but not repaired.

## When to Use
Use this when an ops-prep package is near handoff or result closure and the agent must prove the package is coherent before anyone executes it. Typical triggers are stale front-door or index warnings, uncertain artifact shape, missing `result.md`, stale generated runtime/index/context projections, reusable-operation promotion signals, or a need to ask the repair proposal builder for a typed follow-up.

Do not use it to run a maintenance service, rebuild an index, refresh a projection, write durable knowledge, repair canonical source, execute commands, deploy, seed, migrate, or claim the operation happened. Route those cases to the owning maintenance, operational-execution, hybrid, debug, or durable-domain workflow.

## Source Contract
Grounding sources: `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#operational-and-hybrid-slice-variants`, `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6`, `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19`, and the maintenance-layer anchors in `docs/architecture/master-plugin-target-architecture-part-6e-maintenance-capabilities.html`.

Owning manifest: `capabilities/pipelines/slice-variants/operational-preparation.pipeline.json`, step `step_graph.steps.slice-op-prep-maintenance-check-requester`. The step is optional, invokes this skill, gates on `artifact_and_handoff_readiness_checked`, produces `deferred.md`, and can end as `prepared_not_executed`, `handoff_ready`, or `blocked_missing_preflight`.

No external skill body is referenced for this exact step.

## Operating Procedure
1. Verify the target is an operational-preparation Slice or Result and that the current task is readiness checking, not execution. If authority has shifted to mutation or live operation, stop and route to operational execution or hybrid work.
2. Load the local preparation package shape: operation intent, authority boundary, current state, risk-impact notes, preflight checks, command plan, rollback plan, proof contract, handoff, result, promotion, deferred notes, and any generated projections or indexes already cited by the package.
3. Build the maintenance request set. Include front-door/index freshness, variant-shape check, result-presence check, stale-projection check, promotion-readiness check, and repair-proposal builder when any prior check is likely to return warning, blocked, stale, missing, or repair-required.
4. For each requested check, name the target object, evidence paths already inspected, reason for the request, expected owner, and the exact question to answer. Separate requests that can be answered by read-only inspection from requests that require owner approval or later repair workflow.
5. If maintenance verdicts are already available, classify them without applying fixes: current, warning, stale, missing, blocked, handoff-required, promotion-proposal-needed, or repair-proposal-needed. Maintenance findings are classified but not repaired. Treat stale projections as clues only; stale projections cannot support current-state claims, so reload source truth before any current-state claim.
6. Decide the ops-prep posture. Use `prepared_not_executed` only when required prep artifacts, handoff, proof contract, and result truth are present enough for the next actor. Use `handoff_ready` when the package can move forward but maintenance or owner action remains. Use `blocked_missing_preflight` when required preflight, rollback, proof, result, or source truth is absent.
7. Record the maintenance request and verdict summary in `deferred.md` or the active handoff/result section allowed by the parent workflow. Preserve unresolved findings and route repair, projection refresh, index rebuild, or promotion work to its owner.

## Outputs
The primary output is a compact maintenance-check request packet in `deferred.md`: target Slice/Result identity, requested maintenance checks, evidence paths, stale or missing signals, owner or workflow for each check, and the resulting terminal posture.

If checks are already answered, include a verdict table that distinguishes ready, warning, blocked, and handoff-required states. If a repair is needed, output only a repair-proposal request with issue, evidence, owner, authority need, and expected verification. The skill may feed `handoff.md`, `result.md`, or `promotion.md` by reference, but it does not author final truth for those artifacts.

## Verification
Check that the frontmatter description is trigger-only, the seven required H2 sections are present, the owning manifest and architecture paths are cited, and the procedure names concrete request steps instead of wrapper text.

For runtime use, verify that every requested maintenance check has a target, reason, owner, and expected verdict; that maintenance findings are classified but not repaired; that stale generated projections are not used as current truth because stale projections cannot support current-state claims; and that the output never says the operation was completed. Trigger fixtures must select this skill for ops-prep readiness maintenance requests and reject it for actual maintenance execution, source repair, deployment, root-cause investigation, or durable promotion writes.

## Failure Modes
Block as `blocked_missing_preflight` when required preflight, rollback, proof contract, result boundary, or source-truth input is missing. Use `handoff_ready` when the package is coherent enough for a user or owner but maintenance checks, repair proposals, projection refreshes, or promotion-readiness decisions remain outside the current authority.

Route away when the request asks to apply a repair, refresh an index/projection, write durable knowledge, execute a command, or mutate a target. Escalate to operational execution when authority is granted, to hybrid when code/config changes are needed, to debug when root cause is unknown, and to procedure capture when the prepared operation reveals a reusable procedure candidate.

If the maintenance owner, source path, artifact registry entry, or selected Slice/Result identity is unclear, stop with a handoff or explicit blocker instead of fabricating a verdict. If checks disagree, preserve the contradiction and request repair-proposal routing rather than choosing the most convenient outcome.

If authority is partial or ambiguous, record authorization as unresolved and keep the next action with the owner who can approve maintenance, repair, execution, or promotion.
