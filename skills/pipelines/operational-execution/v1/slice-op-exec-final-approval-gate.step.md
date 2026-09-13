---
id: "slice-op-exec-final-approval-gate"
kind: "internal_step"
owner_family: "slice_variant"
pipeline_id: "slice.operational-execution"
step_id: "slice-op-exec-final-approval-gate"
entry_gate: false
public_trigger: false
native_skill_discovery: false
body_path: "capabilities/instructions/pipelines/slice-variants/operational-execution/slice-op-exec-final-approval-gate.step.md"
source_manifest: "capabilities/pipelines/slice-variants/operational-execution.pipeline.json"
legacy_skill_ref: "slice-op-exec-final-approval-gate"
architecture_refs:
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#operational-and-hybrid-slice-variants"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19"
---

# Operational Execution Final Approval Gate

## Overview
This skill is the final pre-action approval gate before an Operational Execution Slice may move toward command planning, action ledger consumption, or action running. Its core rule: final approval is valid only when authority, target identity, current state, preflight, stop conditions, and action scope are fresh, exact, and explicitly approved by the right actor.

This is not a generic authority declaration. It does not plan commands, run actions, verify checkpoints, roll back, write results, promote durable knowledge, or declare the operation complete.

## When to Use
Use this in `slice.operational-execution` after authority confirmation, current-state baselining, preflight, and risk-stop checks have produced usable evidence, and before the operation is allowed to proceed to an action ledger or action runner.

Use it when consequence, production or live target, destructive potential, rollback limits, policy, or owner rules require a final approval pause. It also applies when a prepared handoff or draft `action-ledger.md` exists and the agent must verify that the proposed action scope still exactly matches the approved operation.

Do not use it for preparation-only work, initial authority capture, current-state discovery, preflight execution, command-ledger construction, command planning, action execution, checkpoint verification, rollback, post-action proof, result writing, promotion, or broad "go ahead" interpretation.

## Source Contract
Ground this skill in `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#operational-preparation-and-operational-execution-variants`, `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#operational-and-hybrid-slice-variants`, `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6`, and `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19`.

The owning manifest is `capabilities/pipelines/slice-variants/operational-execution.pipeline.json`, step `slice-op-exec-final-approval-gate`. The manifest marks this as a required approval step invoking `skill:slice-op-exec-final-approval-gate`, producing `authority-confirmation.md`, gating on `final_approval_confirmed`, failing by `stop_or_handoff`, and reaching `ready_for_next_step` only when approval is current and bounded.

Source inputs are the active Slice contract, `authority-confirmation.md`, `current-state.md`, `preflight.md`, `risk-stop-conditions.md`, and either a proposed action scope or proposed-only `action-ledger.md` draft when one exists. Later variant artifacts such as `execution-log.md`, `post-action-validation.md`, `result.md`, and `recovery-notes.md` belong to downstream owners, not this gate.

This skill defines approval verification behavior only. It does not grant execution authority, execute commands, mutate a workspace, deploy, roll back, promote, or create post-action proof.

## Operating Procedure
1. Confirm the selected path is `slice.operational-execution` and the candidate operation has a bounded target, desired final state, authority record, current-state baseline, preflight verdict, risk-stop record, and proposed action scope or action-ledger input set. If the action already ran, route to checkpoint, post-action, recovery, or result ownership.
2. Verify authority freshness. Read `authority-confirmation.md` for actor, owner, granted authority classes, denied classes, target scope, rollback or recovery authority, timestamp or freshness rule, and evidence source. Treat authority as stale when target, environment, command set, risk, owner, user instruction, time window, or incident context changed after it was recorded.
3. Lock target identity across all gate inputs. The service, repo, branch, host, cluster, environment, account, chain, database, endpoint, or other target identifiers in current state, preflight, risk-stop conditions, and action scope must match exactly or name an approved subtarget. Any mismatch is a stop.
4. Check readiness of the prerequisite artifacts. `current-state.md` must be fresh enough to compare before/after truth, `preflight.md` must be passed or explicitly blocked with no hidden waiver, `risk-stop-conditions.md` must define do-not-continue states and rollback/recovery thresholds, and any draft `action-ledger.md` or action plan must be proposed-only, exact, and not already consumed.
5. Build the exact approval scope from existing source inputs. State the target, actor, allowed action classes, named commands or tool-call families if already proposed, forbidden actions, stop conditions, rollback limits, proof expectations, time window, credential handling, and next manifest owner. Do not invent command planning here, and do not broaden approval from one target, command family, environment, branch, account, or rollback mode to another.
6. Verify user or owner approval evidence. Approval must be explicit, current, and tied to the exact scope from step 5. Record the approving actor, owner role when policy requires one, source message or artifact, timestamp if known, and any condition or denial. Vague approval like "go ahead" is insufficient for production, destructive, financial, security-sensitive, rollback-limited, or policy-gated actions unless the exact scope was restated and accepted.
7. Compare approval against action-ledger readiness. If a draft ledger exists, every row must fit the approved target, authority class, stop condition, timeout, proof expectation, and rollback/recovery boundary. If the ledger is still to be built, record the ledger readiness constraints that the next step must enforce. Any new row, retry, fallback, credential use, target, or proof substitute after approval requires a fresh final approval.
8. Write the final approval decision into `authority-confirmation.md`. Use `ready_for_next_step` only when `final_approval_confirmed` is supported by fresh authority, exact target identity, ready prerequisites, exact scope, explicit user or owner approval, and no implicit broadening. Otherwise write `stop_or_handoff` with the missing, stale, changed, or unapproved gate named.
9. Route the next owner without doing that owner's work. Approved scopes route to command-ledger construction when the action sequence still needs to be built, or to the action runner when a proposed-only ledger was already approved. Failed gates route to user or owner handoff, authority refresh, current-state refresh, preflight, risk-stop repair, rollback-authority request, hybrid escalation, incident escalation, or debug escalation as appropriate.

## Outputs
The required artifact write is an updated `authority-confirmation.md` final-approval section for the active Slice. It must include final approval status, approving actor or owner, approval evidence source, freshness basis, exact target identity, source inputs inspected, prerequisite artifact verdicts, approved action scope, action-ledger readiness constraints, forbidden broadening, rollback or recovery limits, proof expectations, handoff or next-step routing, and terminal decision.

The output may include a short handoff request when the user, service owner, deploy owner, security owner, incident owner, or rollback owner must approve a narrowed scope. It must not create `action-ledger.md`, `execution-log.md`, `post-action-validation.md`, `rollback.md`, `result.md`, or any operation-completed claim.

## Verification
Run `node tools/validate-internal-skill-body-quality.mjs --skill slice-op-exec-final-approval-gate` and `node tools/validate-internal-skill-trigger-fixtures.mjs --skill slice-op-exec-final-approval-gate`. Also parse both fixture JSON files, scan the skill body for exactly these seven H2 sections, and run scoped whitespace and diff checks on this skill plus its two fixture files.

For content verification, inspect the final approval record: authority is fresh, target identity matches across sources, current-state/preflight/risk-stop/action-ledger readiness is explicit, approval evidence names the user or owner and exact scope, implicit broadening is rejected, and the terminal decision is `ready_for_next_step` or `stop_or_handoff`. No command, deployment, rollback, workspace mutation, source mutation, or live-system action may be performed by this skill.

## Failure Modes
Stop or hand off when authority is missing, stale, partial, or target-limited; the approving user or owner is unknown; approval evidence is vague; current-state evidence is stale; preflight failed or was skipped; risk-stop conditions are absent; rollback or recovery authority is required but absent; action-ledger readiness is missing; a draft ledger exceeds approval; target identity changed; a new command, retry, credential, environment, branch, account, or proof substitute appears after approval; or incident/live ownership overrides the Slice.

Keep a zero-action posture for every blocked approval: name the first failed gate, preserve the last valid scope, and hand the decision back to the user or owner instead of narrowing, expanding, or interpreting approval silently.

Also stop when the request asks this skill to perform command planning, run commands, deploy, roll back, mutate files, change branches, verify checkpoints, write results, promote durable knowledge, declare generic authority, or assert completion. Route those cases to the command-ledger builder, action runner, checkpoint verifier, rollback/recovery runner, post-action validator, result writer, promotion router, or a human handoff only after the proper approval gate is satisfied.
