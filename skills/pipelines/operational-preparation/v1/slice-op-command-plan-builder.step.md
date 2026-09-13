---
id: "slice-op-command-plan-builder"
kind: "internal_step"
owner_family: "slice_variant"
pipeline_id: "slice.operational-preparation"
step_id: "slice-op-command-plan-builder"
entry_gate: false
public_trigger: false
native_skill_discovery: false
body_path: "capabilities/instructions/pipelines/slice-variants/operational-preparation/slice-op-command-plan-builder.step.md"
source_manifest: "capabilities/pipelines/slice-variants/operational-preparation.pipeline.json"
legacy_skill_ref: "slice-op-command-plan-builder"
architecture_refs:
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#operational-preparation-and-operational-execution-variants"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19"
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#operational-and-hybrid-slice-variants"
---

# Slice Op Command Plan Builder

## Overview
This Tect-owned reference adapter skill adapts `superpowers:writing-plans` into a preparation-only operations command plan. The core rule is to produce a handoff-ready `operation-plan.md` while preserving `prepared_not_executed` truth: the plan may describe future commands, but this skill must not run them or claim the operation succeeded.

## When to Use
Use this after operational intent, target state baseline inputs, authority boundary, current-state basis, risk-impact notes, and preflight checks are available or explicitly marked missing. It fits deploy, seed, restart, migration, incident workaround, data repair, chain/API operation, or infrastructure tasks where the user wants ordered commands, expected outputs, stop conditions, dry-run/simulation requirements, proof commands, and handoff notes, but execution is not authorized in this Slice.

Do not use this when the next action is direct execution, code/config implementation, root-cause investigation, live validation after an already-run operation, durable runbook promotion, or result writing. Route execution authority to `slice.operational-execution`, code plus live operation to `slice.hybrid-implementation-operation`, unknown cause to `slice.debug-root-cause`, and reusable-procedure capture to the procedure-capture path.

## Source Contract
The owning manifest is `capabilities/pipelines/slice-variants/operational-preparation.pipeline.json`, step `slice-op-command-plan-builder`. It produces `operation-plan.md`, gates on `ordered_commands_or_checklist_defined`, fails as `block_missing_command_plan`, and reaches `operation_plan_ready` only for this step.

Architecture sources:
- `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#operational-preparation-and-operational-execution-variants` defines operational preparation as exact commands, preflight, rollback, proof, and handoff without target mutation.
- `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6` and `#s19` route prep deploy/seed/redeploy work to operational prep, and authorized operations to operational execution.
- `docs/architecture/master-plugin-target-architecture-capability-atom-shard-map.html#pipeline.slice.operational_preparation.op.command.plan.builder` maps this atom to exact commands/tool calls in order with cwd/env/expected output/stop conditions.

Reference source read: `skills/references/superpowers/writing-plans/SKILL.md`. Adapt its implementation plan, task decomposition, expected-output, and verification discipline. Do not import its coding, worktree, TDD, commit, or execution assumptions into this prep-only step.

## Operating Procedure
1. Confirm the baseline packet. Require `operation-intent.md`, `authority-boundary.md`, `current-state.md` or a declared current-state gap, `risk-impact.md`, and `preflight-checks.md`. Extract the operation target, desired final state, non-goals, target state baseline inputs, known current state, risk class, executor, approval needs, and missing authority.
2. Start `operation-plan.md` with a boundary statement: this is a command plan artifact, not an execution log. State that direct execution/deploy/live mutation is forbidden in this step, secret values must not be copied, and any mutating command is for a later authorized actor or variant only.
3. Build an ordered task sequence using writing-plans mechanics. Each task row must have an order number, purpose, command or checklist item, actor, authority/approval need, target host/service/repo/account, cwd, env variable names or secret references without values, preconditions, expected output, timeout or observation window, stop condition, rollback/abort criteria, and proof command or proof source.
4. Order the sequence by operational safety: baseline confirmation, credential/access presence without revealing secrets, read-only checks, dry-run/simulation requirement where the tool supports it, final preflight, future mutating step, immediate checkpoint, rollback trigger check, post-action proof collection, and handoff packaging. If no safe dry-run or simulation exists, write that explicitly and make the missing dry-run a risk or approval note.
5. Separate commands by action class. Mark read-only proof commands as allowed for later validation, dry-run commands as non-mutating only if their expected behavior is known, and mutating commands as "do not run in preparation." Never mix the planned mutating command with proof that assumes it already ran.
6. Tie every future action to proof. For each planned mutation or manual checklist step, specify the verification checkpoint: log query, status endpoint, DB query, version check, API smoke, chain event, UI check, monitoring window, screenshot, or user confirmation. Name the exact proof command when known and expected output or acceptable range.
7. Define abort and rollback gates. State the condition that stops execution before mutation, the signal that stops after a checkpoint, the rollback-plan link or missing rollback blocker, whether rollback authority exists, and when the correct response is to hand off rather than continue.
8. Add terminal-state handling. Use `operation_plan_ready` only when ordered commands or checklist items are complete enough for `slice-op-user-handoff-package-builder`, `slice-op-proof-contract-builder`, and the later result writer. Use `block_missing_command_plan` when target, baseline, authority, cwd/env, command source, expected output, stop condition, dry-run/simulation posture, proof command, or rollback/abort criteria are missing.
9. Route execution explicitly. If the user grants authority during planning, stop and hand off to `slice.operational-execution` or `slice.hybrid-implementation-operation`; do not continue by running the plan. If root cause is unresolved, route to debug before planning commands.

## Outputs
This step owns only `operation-plan.md`. The artifact must include: preparation boundary, target and final-state baseline, assumptions and non-goals, authority owner and approval needs, ordered commands or checklist, cwd/env/secret-reference handling, preconditions, dry-run/simulation requirements, expected outputs, stop conditions, rollback/abort criteria, proof commands, verification checkpoints, downstream links to preflight, rollback, proof-contract, handoff, and result artifacts, and a final `operation_plan_ready` or `block_missing_command_plan` verdict.

The output may reference `rollback-plan.md`, `proof-contract.md`, `handoff.md`, and `result.md` as downstream artifacts, but it does not own them. It must preserve `prepared_not_executed` truth and must not create `execution-log.md`, `action-ledger.md`, `post-action-validation.md`, `authority-confirmation.md`, deployment proof, live proof, durable runbook promotion, or an operation-completed claim.

## Verification
Verify the plan against the manifest gate `ordered_commands_or_checklist_defined`. Every action row must have sequence order, task purpose, actor, authority/approval need, cwd/env posture, preconditions, expected output, timeout or window when relevant, stop condition, rollback/abort criteria, and proof command or proof source. The text must visibly adapt writing-plans mechanics by using an implementation plan style, task decomposition, and verification checkpoints for an operational command plan.

Verify the preparation boundary separately: no target operation command was run, no secret value was copied, no direct execution/deploy/live mutation authorization was granted by this skill, no operation-completed claim appears, and every execution request is routed to operational execution or hybrid before mutation. Source validation commands are `node tools/validate-internal-skill-body-quality.mjs --skill slice-op-command-plan-builder` and `node tools/validate-internal-skill-trigger-fixtures.mjs --skill slice-op-command-plan-builder`.

## Failure Modes
Block with `block_missing_command_plan` when the target, desired final state, current-state baseline, authority owner, approval need, command source, cwd/env, dry-run/simulation posture, expected output, stop condition, rollback/abort criteria, or proof command is unknown. Do not invent commands for production, irreversible, credential-sensitive, security-sensitive, data-loss, or live-incident operations.

Hand off instead of planning when execution authority is already granted and the plan is ready to run, implementation work is needed before the operation can exist, root cause is still unknown, preflight is failing, rollback authority is missing for a risky operation, or the requested outcome requires live validation. Terminal states remain `operation_plan_ready` for this step, `block_missing_command_plan` for incomplete command planning, and later `prepared_not_executed`, `handoff_ready`, `blocked_missing_authority`, or `escalated_to_operational_execution` after downstream operational-preparation steps.
