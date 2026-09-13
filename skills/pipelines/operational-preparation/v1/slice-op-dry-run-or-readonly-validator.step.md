---
id: "slice-op-dry-run-or-readonly-validator"
kind: "internal_step"
owner_family: "slice_variant"
pipeline_id: "slice.operational-preparation"
step_id: "slice-op-dry-run-or-readonly-validator"
entry_gate: false
public_trigger: false
native_skill_discovery: false
body_path: "capabilities/instructions/pipelines/slice-variants/operational-preparation/slice-op-dry-run-or-readonly-validator.step.md"
source_manifest: "capabilities/pipelines/slice-variants/operational-preparation.pipeline.json"
legacy_skill_ref: "slice-op-dry-run-or-readonly-validator"
architecture_refs:
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#operational-and-hybrid-slice-variants"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19"
---

# slice-op-dry-run-or-readonly-validator

## Overview

This skill validates whether an operational-preparation Slice can collect non-mutating dry-run or read-only evidence. It decides which planned checks are feasible as `read_only_probe`, which are true `dry_run_simulation`, which are `live_mutation`, and which are `unknown_effect` gaps that must stop, skip, or hand off.

It produces only `dry-run.md` and optional scrubbed `evidence/` for `capabilities/pipelines/slice-variants/operational-preparation.pipeline.json`. The highest truth remains preparation truth: `readonly_validation_recorded` for safe validation evidence or `prepared_not_executed` when no safe validation can be retained. It must not execute side-effect operations, mutate workspace/source/live systems, grant authority, claim operation proof, or close the operation result.

## When to Use

Trigger this skill when all of these are true:

- Runtime has selected `slice.operational-preparation`, the operation is not authorized for execution, and the optional step is `slice-op-dry-run-or-readonly-validator`.
- Existing prep artifacts name read-only checks, status checks, plan previews, no-apply commands, dry-run flags, or proof gaps that need classification before handoff.
- The next useful output is a feasibility verdict for safe validation, not a command plan, execution log, rollback, post-action proof, or final result.
- A proposed check might touch production, credentials, data, chain state, infra, deployment tooling, migrations, seeds, deletes, or writes, and its effect class must be separated before anyone runs it.

Do not select this skill for preflight checklist design, command-plan authoring, mutating execution, final approval, rollback/recovery execution, post-action validation, live deployment proof, result writing, durable runbook promotion, code/config repair, or unknown-root-cause debugging. Route those to `slice-op-preflight-check-builder`, `slice-op-command-plan-builder`, `slice.operational-execution`, `slice.hybrid-implementation-operation`, `slice.debug-root-cause`, or the relevant result/promotion step.

## Source Contract

Classification: `skill_body`.

Ground behavior in:

- `capabilities/pipelines/slice-variants/operational-preparation.pipeline.json`, step `slice-op-dry-run-or-readonly-validator`, which produces `dry-run.md` and `evidence/`, gates on `non_mutating_validation_only`, and fails by `skip_or_block_mutating_check`.
- The operational-preparation artifact contract in the same manifest, which allows optional `dry-run.md` and `evidence/` but forbids `execution-log.md`, `action-ledger.md`, `post-action-validation.md`, `authority-confirmation.md`, and execution-run folders.
- `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#operational-and-hybrid-slice-variants`, where operational preparation packages safe operation details without target mutation.
- `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#operational-preparation-and-operational-execution-variants`, where preparation starts when commands, checklist, preflight, rollback, proof criteria, or handoff are needed but execution is not authorized.
- `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6`, where operational preparation is a Slice variant with ready, blocked, or handoff completion boundary.
- `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19`, where prep deploy, seed, and redeploy requests select operational prep until authority changes.
- `docs/architecture/master-plugin-target-architecture-capability-atom-shard-map.html#pipeline.slice.operational_preparation.op.dry.run.or.readonly.validator`, whose atom row maps this manifest step to Tect work, approval-required authority, workspace-control-plane storage, and the summary "run allowed read-only or dry-run checks and record outputs without mutating target."

Required source inputs are `operation-intent.md`, `authority-boundary.md`, `current-state.md`, `risk-impact.md`, `preflight-checks.md`, `operation-plan.md`, `rollback-plan.md`, `proof-contract.md`, and any user-supplied source proving a dry-run or read-only mode. If a source is missing, stale, contradictory, secret-bearing, or not authoritative for the proposed command, record a proof gap before validation.

## Operating Procedure

1. Confirm the selected Runtime context: `slice.operational-preparation`, prep-only authority, this step id, and the `non_mutating_validation_only` gate. If the current request grants execution authority or asks for post-action proof, route out before classifying checks.
2. Load source inputs and name their freshness. Record every missing source, stale current-state baseline, ambiguous authority boundary, or missing proof contract as a proof gap.
3. Reconstruct target identity: environment, service, repo/worktree, branch, host, database, chain, account, URL, namespace, credential class, or other bounded target handle. If the target cannot be uniquely identified, stop with handoff required.
4. Separate current state, expected final state, and prep-observable state. A prep-only check may observe readiness, access, plan output, status, drift, or proof gaps; it cannot prove the operation completed.
5. Extract proposed validation items from `preflight-checks.md`, `operation-plan.md`, `rollback-plan.md`, `proof-contract.md`, user instructions, and runbook snippets. Preserve each check id, command/tool shape, target, owner, expected output, stop condition, and source citation.
6. Classify every proposed item:
   - `read_only_probe`: observes state through a GET/status/list/log/query/inspection command that is documented or locally known not to write.
   - `dry_run_simulation`: invokes a tool mode such as check, plan, diff, preview, validate, explain, or dry-run that explicitly avoids apply, commit, deploy, seed, migrate, delete, publish, or send.
   - `live_mutation`: can create, update, delete, deploy, migrate, seed, transfer, publish, restart, acknowledge, mark processed, write cache/state, or trigger a side effect.
   - `unknown_effect`: lacks enough source truth to prove it is non-mutating.
7. Apply feasibility gates to every retained item: exact target identity, explicit read or dry-run authority, non-mutating source proof, credential boundary without secret value, freshness requirement, expected evidence, no-write proof phrase or status, timeout, stop condition, and evidence retention path.
8. Do not execute any item that can mutate workspace files, source repos, branches, worktrees, package state, databases, queues, live services, deployments, chain state, credentials, traffic, or durable knowledge. Do not stage, commit, push, install, deploy, apply, migrate, seed, restart, rollback, send, publish, acknowledge, mark processed, or promote.
9. For each feasible `read_only_probe`, record the exact command or tool call, cwd/target, credential class, expected safe output, freshness rule, timeout, and stop condition. It may be run only if the surrounding Runtime and user authority separately permit that specific non-mutating probe; otherwise record it as feasible but not run.
10. For each feasible `dry_run_simulation`, record the no-apply flag or mode, the source proving it is non-mutating, expected output, and the phrase or status that proves no write happened. Undocumented dry-run behavior remains `unknown_effect`.
11. Move every `live_mutation` and unresolved `unknown_effect` item out of the validation set. Mark it `skip_or_block_mutating_check`, `handoff_required`, or `route_to_operational_execution_after_authority`; route to hybrid when code/config changes are required before validation can mean anything.
12. Write `dry-run.md` with per-check verdicts, evidence pointers, residual risk, proof gaps, stop/handoff conditions, and terminal state. Keep operation result closure for later result steps.

## Outputs

Produce or update only `dry-run.md` and optional scrubbed files under `evidence/`.

Minimum `dry-run.md` shape:
- `target_identity`: exact environment, service, repo/worktree, branch, host, database, chain, account, URL, namespace, or bounded target.
- `source_inputs`: artifacts read, timestamps or freshness labels, and source class.
- `authority_posture`: prep-only boundary, read authority, dry-run authority, missing authority, and actor ownership.
- `state_model`: current state, expected final state, and prep-observable facts.
- `validation_items`: ordered table with check id, command/tool shape, target, source proof, effect class, authority requirement, expected evidence, stop condition, and verdict.
- `allowed_read_only_probes`: feasible or separately authorized non-mutating checks.
- `allowed_dry_run_simulations`: feasible or separately authorized no-apply simulations.
- `blocked_or_skipped_items`: `live_mutation`, `unknown_effect`, missing authority, stale baseline, or unsafe credential cases.
- `proof_gaps`: missing target identity, stale current state, unclear expected state, untrusted dry-run mode, unavailable read-only access, or evidence that cannot distinguish preview from apply.
- `residual_risk`, `handoff_or_escalation_route`, and `terminal_state`.

`evidence/` may contain copied command output, screenshots, API responses, log snippets, or local inspection notes only when they are scrubbed for secrets and tied to one validation item. Evidence must say whether it came from a read-only probe, dry-run simulation, user-provided proof, or skipped check. Do not create execution logs, action ledgers, post-action validation, authority confirmation, result closure, promotion records, durable-domain writes, or operation-completed claims.

Allowed terminal states are `readonly_validation_recorded` when at least one safe validation item has usable proof, and `prepared_not_executed` when the package remains preparation-only, blocked, or handed off. Never emit a completed, deployed, rolled back, live-verified, promoted, or operation-proof terminal claim.

## Verification

Proof gates before handoff:
- The selected variant is operational preparation and this step is optional validation, not execution.
- Every retained check has target identity, non-mutating source proof, authority requirement, expected evidence, stop condition, and evidence path.
- Every `live_mutation` or `unknown_effect` check is skipped, blocked, or routed out.
- `dry-run.md` separates read-only probes, dry-run simulations, live mutations, authority requirements, target identity, current state, expected state, proof gaps, stop/handoff conditions, and terminal state.
- No output claims execution, deployment, rollback, seed, migration, transfer, promotion, live validation, or operation completion.

For source validation after changing this skill or its fixtures, run:
- `node tools/validate-internal-skill-body-quality.mjs --skill slice-op-dry-run-or-readonly-validator`
- `node tools/validate-internal-skill-trigger-fixtures.mjs --skill slice-op-dry-run-or-readonly-validator`

Also parse both JSON fixtures, confirm exactly the seven required H2 sections are present, check final newline and trailing whitespace on owned files, and run path-scoped `git diff --check`.

## Failure Modes

Use `prepared_not_executed` with a blocker or handoff when target identity is ambiguous, expected state is missing, current-state baseline is stale, authority is absent, source proof is weak, a dry-run mode is undocumented, a read-only probe requires unauthorized credentials, evidence would reveal secrets, or the proposed validation could mutate target state.

Block the mutating check when a tool performs apply-like behavior during preview, writes generated state that matters, opens a transaction with side effects, restarts services, sends messages, touches production data, updates queues, changes traffic, or depends on irreversible external systems.

Route missing preflight definitions to `slice-op-preflight-check-builder`, missing command shape to `slice-op-command-plan-builder`, missing proof criteria to `slice-op-proof-contract-builder`, manual evidence collection to `slice-op-user-handoff-package-builder`, explicit execution authority to `slice.operational-execution`, code/config prerequisites to `slice.hybrid-implementation-operation`, and unknown root cause to `slice.debug-root-cause`. Do not hide a failed or unsafe check by relabeling it optional after it blocks.
