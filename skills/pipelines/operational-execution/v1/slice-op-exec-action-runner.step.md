---
id: "slice-op-exec-action-runner"
kind: "internal_step"
owner_family: "slice_variant"
pipeline_id: "slice.operational-execution"
step_id: "slice-op-exec-action-runner"
entry_gate: false
public_trigger: false
native_skill_discovery: false
body_path: "capabilities/instructions/pipelines/slice-variants/operational-execution/slice-op-exec-action-runner.step.md"
source_manifest: "capabilities/pipelines/slice-variants/operational-execution.pipeline.json"
legacy_skill_ref: "slice-op-exec-action-runner"
architecture_refs:
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#operational-and-hybrid-slice-variants"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19"
---

# Operational Action Runner

## Overview

This skill performs the action step of an Operational Execution Slice after authority, preflight, stop conditions, final approval, and an ordered action ledger already exist. It never creates authority, expands scope, or converts preparation into execution; it consumes the exact approved ledger entry and records what happened.

## When to Use

Use this when Runtime has selected `slice.operational-execution`, `authority-confirmation.md` records explicit execute authority, `risk-stop-conditions.md` is current, and `action-ledger.md` names the next exact command or tool call. The action must have an exact target identity: system, environment, repo/worktree, account, service, chain, database, branch, host, or other bounded target named in the Slice contract.

Do not use this for preparation-only work, command planning, approval gathering, checkpoint verification, post-action validation, rollback execution, incident response outside the Slice, implementation work, or any action whose target, command text, credentials, environment, or proof expectation is ambiguous.

## Source Contract

Grounding: `capabilities/pipelines/slice-variants/operational-execution.pipeline.json` step `slice-op-exec-action-runner`, which produces `execution-log.md`, gates on `action_modeled_or_runtime_authorized`, fails by `stop_or_handoff`, and advances to `ready_for_next_step`. Architecture sources: `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#operational-preparation-and-operational-execution-variants`, `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#operational-and-hybrid-slice-variants`, `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6`, and `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19`.

This skill is a `skill_body`: the manifest and architecture define a real action-running procedure. It may operate only inside an active, authority-gated Runtime execution context; the skill text alone grants no workspace, deployment, live-system, rollback, durable-domain, branch, package, or promotion authority.

## Operating Procedure

1. Re-read the active Slice contract, final approval record, risk-stop conditions, and next unconsumed action-ledger row. Confirm they all name the same target identity and that the ledger row is the next step, not a skipped or reordered step.
2. Compare the requested command or tool call byte-for-byte against the ledger row, including cwd, target, account/environment, timeout, expected proof, redaction rules, and allowed secret handling. If any part differs, stop before action.
3. Evaluate stop conditions immediately before action: authority expired, target changed, preflight is stale, service health regressed, user narrowed scope, required rollback posture is missing, checkpoint from the prior step is absent, or the command would touch an unapproved target.
4. Perform exactly one ledger item. Do not batch later items. No hidden retries: do not retry, substitute flags, switch targets, add cleanup commands, change credentials, or run an alternate tool unless that exact retry or alternate path is already modeled and approved in the ledger.
5. Capture proof for `execution-log.md`: timestamp, actor, target identity, ledger row id, exact input, cwd, redacted environment, exit status or tool result, stdout/stderr or returned artifact summary, produced paths, elapsed time, deviations, and whether expected proof appeared.
6. After the item, evaluate its checkpoint rule. If a checkpoint is required, stop and route to `slice-op-exec-checkpoint-verifier`; do not continue to the next action until the checkpoint verifier records `checkpoints_verified_or_stopped`.
7. If the action fails, partially succeeds, touches an unexpected target, triggers a stop condition, or needs rollback, stop immediately. Route to rollback or recovery only when rollback authority is explicit; otherwise produce a blocked recovery handoff.
8. Do not claim completion. The highest allowed success statement is that this ledger item was attempted and logged. Completion waits for `slice-op-exec-post-action-validator`, result writing, and the declared proof level.

## Outputs

Primary output is an `execution-log.md` entry tied to the parent Slice and ledger row. It must include exact target identity, command/tool identity, redacted inputs, observed output, exit status, proof captured, checkpoint requirement, stop-condition evaluation, and next route.

Terminal state is `ready_for_next_step` only when the item matched the approved ledger, no stop condition fired, and the log is complete. Otherwise emit `stop_or_handoff` with the blocking reason, missing authority or proof, and the recommended next owner: checkpoint verifier, post-action validator, rollback/recovery runner, user handoff, or incident-specific route.

## Verification

Before action, verify the final approval, ledger row, target identity, preflight freshness, and stop-condition list all agree. During action, verify that the exact command or tool call is the one being performed and that sensitive values are redacted in durable records. After action, verify the log includes exit status or tool result, evidence location, deviation notes, and checkpoint routing.

The action runner is green only when it can show a complete `execution-log.md` row for the exact ledger item. It is not green because the command returned zero, a deploy job started, a service looks healthy, or an operator believes the result worked. Post-action validation remains a separate required proof gate.

## Failure Modes

Stop before action when authority is missing, final approval is absent, target identity differs across artifacts, the ledger row is ambiguous, preflight is stale, rollback posture is required but missing, or the command would expand scope. Stop during or after action when output differs from expectation, the target is not the approved target, a stop condition triggers, the next action would be a hidden retry, or proof is missing.

If rollback is needed but not authorized, do not improvise. Record a blocked recovery handoff with the failed ledger row, observed state, safest known pause point, required human approval, and proof needed before any next action. Never hide a failed, partial, or unvalidated operation behind a completion claim.
