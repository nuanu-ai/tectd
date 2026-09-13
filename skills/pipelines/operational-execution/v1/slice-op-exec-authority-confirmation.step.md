---
id: "slice-op-exec-authority-confirmation"
kind: "internal_step"
owner_family: "slice_variant"
pipeline_id: "slice.operational-execution"
step_id: "slice-op-exec-authority-confirmation"
entry_gate: false
public_trigger: false
native_skill_discovery: false
body_path: "capabilities/instructions/pipelines/slice-variants/operational-execution/slice-op-exec-authority-confirmation.step.md"
source_manifest: "capabilities/pipelines/slice-variants/operational-execution.pipeline.json"
legacy_skill_ref: "slice-op-exec-authority-confirmation"
architecture_refs:
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#operational-and-hybrid-slice-variants"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19"
---

# Operational Execution Authority Confirmation

## Overview
This skill is the executable-authority approval pause for `slice.operational-execution`. It records whether a specific actor may proceed toward a specific operational action on a bounded target under declared command, approval, stop-condition, credential, proof, and rollback limits.

The gate is deliberately zero-action. It confirms authority and writes `authority-confirmation.md`; it does not run the operational command, build hidden approval, mutate the workspace, deploy, roll back, promote, or claim the operation succeeded.

## When to Use
Use this after the Operational Execution Slice has a bounded operation target and before any mutating command, deploy, seed, migration, rollback, recovery, live-system action, or other side-effecting tool call may proceed. It is especially required when Runtime is moving from an operation plan, prepared handoff, action scope, or draft `action-ledger.md` toward execution and must re-confirm executable authority.

Use it to distinguish read, write, execute, deploy, promote, rollback or recovery, live validation, and credential-handling authority for the exact operation. If the selected path is preparation-only, implementation plus live operation, unknown-cause debugging, procedure capture, or a later proof/result/promotion step, route to that owning variant or manifest step instead.

Do not use it to perform context loading, baselining, preflight, risk modeling, command ledger construction, action running, checkpoint verification, rollback, post-action validation, result writing, or durable promotion.

## Source Contract
Ground this skill in `capabilities/pipelines/slice-variants/operational-execution.pipeline.json`, step `slice-op-exec-authority-confirmation`. The manifest declares this step as required `approval`, invokes `skill:slice-op-exec-authority-confirmation`, produces `authority-confirmation.md`, gates on `explicit_execute_authority_confirmed`, fails by `stop_or_handoff`, and reaches `ready_for_next_step` only as an authority-confirmation result.

Architecture sources are `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#operational-preparation-and-operational-execution-variants`, `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#operational-and-hybrid-slice-variants`, `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6`, and `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19`. The atom row is `pipeline.slice.operational_execution.authority.confirmation` in `docs/architecture/master-plugin-target-architecture-capability-atom-shard-map.html`.

The current manifest step does not own command execution. Execution belongs to `slice-op-exec-action-runner`, rollback or recovery belongs to `slice-op-exec-rollback-or-recovery-runner`, and proof/result/promotion belong to their later steps. If a future manifest occurrence assigns different ownership, obey that manifest occurrence; this operational-execution occurrence is confirmation only.

## Operating Procedure
1. Load the source inputs that exist: parent Slice id, `slice.md`, `operation-intent.md`, prepared handoff or `source-operation-plan.md`, current authority instruction, optional prior `authority-confirmation.md`, optional `current-state.md`, optional `preflight.md`, optional `risk-stop-conditions.md`, optional draft `action-ledger.md`, known credential constraints, and any explicit user or owner approval text. Missing later-step inputs are not invented; they become routing constraints.
2. Confirm the selected variant and target. The path must be `slice.operational-execution`; the target system, environment, repo or worktree, branch, service, account, host, cluster, database, chain, endpoint, deployment surface, or other target identity must be bounded. Authority for one target does not transfer to another.
3. Re-state the executable action scope. Record the planned command or tool-call family when known, the allowed command text or command class, cwd or execution surface when relevant, actor, target, desired final state, allowed time window, expected proof, and forbidden substitutions. If the command is not yet ledgered, record the exact constraints that `slice-op-exec-command-ledger-builder` must enforce.
4. Build the authority matrix. Include separate rows for read, write, execute, deploy, promote, rollback, recovery, live validation, and credential handling. Each row must state `granted`, `denied`, `unknown`, or `requires_human`; the actor allowed to perform it; source evidence; scope limit; freshness; and whether the row is required before any action.
5. Apply the exact re-confirmation checklist:
   - command gate: command or tool-call family is named or constrained, no hidden retries or substitutions;
   - target gate: every source names the same bounded target or approved subtarget;
   - actor gate: the executor is named and allowed for each granted authority row;
   - approval gate: user, owner, incident, deploy, security, or rollback approval is explicit and current for this scope;
   - stop gate: stop conditions and do-not-continue states are known or routed to the risk-stop owner;
   - rollback gate: rollback or recovery authority, owner, limits, and proof expectations are explicit before risky execution.
6. Reject ambiguous authority. Phrases like "do it", "go ahead", or "you can handle it" are not enough for production, destructive, financial, security-sensitive, irreversible, credentialed, rollback-limited, deploy, migration, seed, delete, branch, or durable-promotion actions unless the exact scope was restated and accepted.
7. Decide the next route without executing. If authority is explicit but prerequisite inputs are still missing, route to the next missing manifest owner: context loader, contract writer, current-state baseliner, preflight runner, risk-stop checker, final-approval gate, or command-ledger builder. Route to action runner only when Runtime already has the required confirmation, approval, ledger, preflight, and stop-condition records. Never perform the command in this skill.
8. Write `authority-confirmation.md` and end with `ready_for_next_step` only when `explicit_execute_authority_confirmed` is true for the bounded operation and every required authority row is non-ambiguous. Otherwise write `stop_or_handoff` with the missing authority, stale scope, target mismatch, actor gap, approval gap, stop-condition gap, rollback gap, or credential gap named.

## Outputs
Primary output: `authority-confirmation.md`.

The record must include: Slice id; selected variant; operation target; desired final state; source inputs reviewed; authority source and freshness; command or tool-call scope; target identity; actor and owner; approval evidence; authority matrix rows for read, write, execute, deploy, promote, rollback, recovery, live validation, and credential handling; granted and denied actions; stop conditions or routing gap; rollback or recovery owner and limits; credential handling rules; proof expectations; forbidden broadening; next route; and terminal decision.

Allowed terminal decisions are `ready_for_next_step` and `stop_or_handoff`. The output may include a handoff request to the user, deploy owner, incident owner, security owner, service owner, or rollback owner. It must not create `action-ledger.md`, `execution-log.md`, `rollback.md`, `post-action-validation.md`, `result.md`, `promotion.md`, or any operation-completed claim.

## Verification
Run `node tools/validate-internal-skill-body-quality.mjs --skill slice-op-exec-authority-confirmation` and `node tools/validate-internal-skill-trigger-fixtures.mjs --skill slice-op-exec-authority-confirmation`. Also parse both owned fixture JSON files and run `git diff --check` for this skill and its two fixtures.

For content verification, inspect the proposed `authority-confirmation.md`: source inputs are named; command, target, actor, approval, stop, rollback, credential, and proof gates are explicit; every required authority class has `granted`, `denied`, `unknown`, or `requires_human`; `explicit_execute_authority_confirmed` is true only for the exact bounded operation; missing or stale authority leads to `stop_or_handoff`; and the next route is a manifest owner rather than an action performed by this skill.

Confirm the body cites the operational-execution manifest, Part 6B operational execution and hybrid anchors, final map `#s6` and `#s19`, and atom row `pipeline.slice.operational_execution.authority.confirmation`. Confirm it does not claim live proof, execution, rollback, deployment, workspace/source mutation, durable promotion, or completion.

## Failure Modes
Stop or hand off when the selected variant is not `slice.operational-execution`; the target is unbounded; command scope is absent or has unapproved substitutions; actor ownership is unclear; approval is vague, stale, or for another target; execute authority is only implied; read authority is granted but write, deploy, rollback, recovery, credential, or promote authority is unknown; stop conditions are absent for risky work; rollback authority is missing when rollback is required; credential handling is unclear; preflight or current-state prerequisites are missing without a safe next route; or incident ownership overrides the generic Slice path.

Also stop when the request asks this skill to run a command, deploy, seed, migrate, delete, change branches, mutate files, call a live system, roll back, promote durable knowledge, or assert completion. Preserve the last safe target and route to the correct next manifest owner or human actor instead of interpreting, expanding, or executing authority silently.
