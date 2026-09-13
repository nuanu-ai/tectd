---
id: "slice-op-authority-boundary-declarer"
kind: "internal_step"
owner_family: "slice_variant"
pipeline_id: "slice.operational-preparation"
step_id: "slice-op-authority-boundary-declarer"
entry_gate: false
public_trigger: false
native_skill_discovery: false
body_path: "capabilities/instructions/pipelines/slice-variants/operational-preparation/slice-op-authority-boundary-declarer.step.md"
source_manifest: "capabilities/pipelines/slice-variants/operational-preparation.pipeline.json"
legacy_skill_ref: "slice-op-authority-boundary-declarer"
architecture_refs:
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#operational-and-hybrid-slice-variants"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19"
---

# Slice Op Authority Boundary Declarer

## Overview
This skill records the authority boundary for an operational-preparation Slice. The core rule is: preparation may clarify, package, and hand off an operation, but target mutation belongs to an explicitly authorized later actor or another Slice variant. It declares who owns each authority lane; it does not grant permission, run operations, validate live success, clean up targets, or promote durable knowledge.

## When to Use
Use when `slice.operational-preparation` has captured an operation target and desired final state, and the next steps need to know whether the user, a team member, the agent later, or nobody yet may execute, deploy, write, roll back, clean up, promote, or validate live state.

Use it for requests such as exact commands, preflight checklists, rollback plans, proof criteria, or handoff packages where execution is not authorized yet. Do not use it when execution authority is already explicit and bounded; route to `slice.operational-execution`. Do not use it when code/config changes are required before the operation can exist; route to `slice.hybrid-implementation-operation`. If the requested operation repairs an unknown cause, route to debug before operational preparation continues.

## Source Contract
Grounding:
- `capabilities/pipelines/slice-variants/operational-preparation.pipeline.json`, step `slice-op-authority-boundary-declarer`, is a required approval step invoking this skill, producing `authority-boundary.md`, gating on `read_prep_execute_deploy_write_separated`, failing with `block_missing_authority_boundary`, and ending in `authority_boundary_recorded` or `blocked_missing_authority`.
- `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#operational-and-hybrid-slice-variants` defines operational preparation as exact safe operation packaging without target mutation and names this skill as the point that states executor and separates read/prep from execute/deploy/write.
- `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6` keeps Slice/result proof boundaries explicit; `#s19` selects operational preparation when the user wants commands, checklist, preflight, rollback, proof, or handoff without execution.
- `docs/architecture/master-plugin-target-architecture-capability-atom-shard-map.html` maps `pipeline.slice.operational_preparation.op.authority.boundary.declarer` as an accepted manifest step with approval-required authority.

No external skill body is part of this step. This skill declares authority state; it does not grant permissions, perform operations, run cleanup, promote durable artifacts, or make later proof claims.

## Operating Procedure
1. Confirm the active work is operational preparation: target and desired final state are known, prep-only boundaries are still in force, and no one has asked the agent to mutate the target now. If that is false, route before writing an authority boundary.
2. Build the authority dimensions for `authority-boundary.md`: read inspection, local prep artifacts, dry-run or read-only validation, command execution, source or remote writes, deploy/apply/migrate/seed/delete, rollback or recovery, live validation, cleanup or retirement, durable promotion, and handoff.
3. For each dimension, record the actor and scope: user, named team or external operator, agent later after explicit approval, CI/tooling owner, maintenance owner, domain owner, or blocked. Include target environment, path, service, chain, repo, branch, account, artifact, or durable domain scope when known.
4. Separate authority from access. Shell access, credentials, writable files, prior session behavior, or a generated command plan are not approval. Treat silence, stale permission, vague "you can handle it", unknown production target, missing rollback owner, missing cleanup owner, unclear promotion target, or unclear credential boundary as missing authority.
5. Mark each dimension as `prep_allowed`, `read_only_allowed`, `approval_required`, `handoff_required`, `blocked`, or `refresh_required`. The gate is satisfied only when read/prep authority and execute/deploy/write authority are distinct enough for later planning.
6. Declare stop conditions for later artifacts. Include any state that must stop planning, execution, rollback, live validation, cleanup, or promotion, such as missing owner, protected target, stale authority, failed preflight dependency, unsafe rollback, secret exposure risk, ambiguous production boundary, or proof source unavailable.
7. Add next-step routing. Execution approval now routes to `slice.operational-execution`; code/config plus operation routes to `slice.hybrid-implementation-operation`; unclear root cause routes to debug; reusable operation capture routes to procedure/runbook promotion only as a candidate; missing actor or approval ends in `blocked_missing_authority`.
8. End with a verdict. Use `authority_boundary_recorded` only when a later step can plan commands, preflight, rollback, proof, cleanup, promotion, or handoff without confusing preparation with execution. Otherwise use `blocked_missing_authority` and list the exact missing approval, actor, target, owner, or freshness check.

## Outputs
Produce `authority-boundary.md` with this shape:

- Operation target, desired final state, environment/scope, and non-goals.
- Source of stated authority, authority freshness, expiration, and any approval questions.
- Actor and owner map covering user, team/operator, agent later, CI/tooling, rollback, live-validation, cleanup, promotion, and handoff ownership.
- Authority matrix with one row per dimension: read, prep artifact writes, dry-run/read-only validation, execute, write, deploy/apply/migrate/seed/delete, rollback/recovery, live validation, cleanup/retirement, durable promotion, and handoff. Each row names owner, scope, status, approval source, stop condition, and next route.
- Denied or out-of-scope actions, forbidden assumptions, gate verdict for `read_prep_execute_deploy_write_separated`, terminal state, and next-step routing.
- Proof obligations affected by authority: what later `proof-contract.md`, `handoff.md`, or `result.md` may claim and what they must forbid.

This step influences later `operation-plan.md`, `rollback-plan.md`, `proof-contract.md`, `handoff.md`, and `result.md`, but it owns only the authority boundary. The output must preserve `prepared_not_executed` truth and must not contain an execution log, action ledger, deploy proof, live proof, rollback proof, or operation-completed claim.

## Verification
Validate the skill body with `node tools/validate-internal-skill-body-quality.mjs --skill slice-op-authority-boundary-declarer` and trigger scenarios with `node tools/validate-internal-skill-trigger-fixtures.mjs --skill slice-op-authority-boundary-declarer`.

For actual use, inspect `authority-boundary.md` against the manifest gate `read_prep_execute_deploy_write_separated`: read/prep permission is explicit, execute/deploy/write authority is separately explicit or denied, cleanup and promotion authority are separately owned or blocked, actor ownership is named, stop conditions are present, stale or vague approval is treated as missing, and escalation is chosen when preparation is no longer the right variant. Also verify that no target mutation was performed, no cleanup or promotion was performed, no secret value was exposed, and no completion/proof claim exceeds the recorded authority.

## Failure Modes
Block with `blocked_missing_authority` when the actor, target environment, protected branch or production boundary, credential owner, execution owner, deploy/write owner, rollback owner, live-validation owner, cleanup owner, promotion owner, or approval freshness cannot be established. Block rather than inferring permission from available tools, previous sessions, or broad conversational language.

Escalate instead of continuing when authority is now execution-grade, when implementation and operation are coupled, or when root cause is unknown. Hand off when the user or another operator must approve or run the future operation. Route durable reuse to procedure/runbook capture only as a candidate, not as a promotion action. Do not let this skill downgrade authority conflicts into assumptions, and do not treat `authority_boundary_recorded` as proof that the operation ran, cleaned up, promoted, or succeeded.

Final zero-action posture: if the boundary cannot be made explicit, leave the Slice prepared or blocked with the next approval question, not partially authorized by implication.
