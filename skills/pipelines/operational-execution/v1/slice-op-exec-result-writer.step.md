---
id: "slice-op-exec-result-writer"
kind: "internal_step"
owner_family: "slice_variant"
pipeline_id: "slice.operational-execution"
step_id: "slice-op-exec-result-writer"
entry_gate: false
public_trigger: false
native_skill_discovery: false
body_path: "capabilities/instructions/pipelines/slice-variants/operational-execution/slice-op-exec-result-writer.step.md"
source_manifest: "capabilities/pipelines/slice-variants/operational-execution.pipeline.json"
legacy_skill_ref: "slice-op-exec-result-writer"
architecture_refs:
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#operational-preparation-and-operational-execution-variants"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19"
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#operational-and-hybrid-slice-variants"
---

# Slice Operational Execution Result Writer

## Overview
This skill closes an Operational Execution Slice by recording `result.md` from evidence already gathered by authority, preflight, action, validation, observation, rollback, and recovery steps. The core rule is: action execution is not completion; `result.md` states the highest validated truth reached and never turns an action log, desired target state, preflight success, local evidence, or missing proof into live completion.

## When to Use
Use this when Runtime selected `slice.operational-execution`, the active step is `slice-op-exec-result-writer`, and the operational Slice has reached closure after authorized action, post-action validation, observation, rollback/recovery, or an explicit block.

Use it when the next artifact is `result.md` and the decision is whether the truthful terminal state is `executed_with_declared_proof_level`, `rolled_back_verified`, `stopped_by_stop_condition`, `blocked_missing_authority`, `blocked_missing_preflight`, `blocked_missing_post_action_proof`, or `handoff_ready`.

Do not use it to grant authority, run commands, mutate a target, gather fresh live proof, perform rollback, write recovery notes, decide promotion, update durable domains, or repair missing upstream artifacts. Route to the owning operational execution step when those actions are still needed.

## Source Contract
Grounding sources are `capabilities/pipelines/slice-variants/operational-execution.pipeline.json` step `slice-op-exec-result-writer`, `capabilities/pipelines/slice-variants/operational-execution.pipeline.json#step_graph.steps.slice-op-exec-result-writer.invokes.slice-op-exec-result-writer`, `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#operational-preparation-and-operational-execution-variants`, `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#operational-and-hybrid-slice-variants`, `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6`, `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19`, and `docs/architecture/master-plugin-target-architecture-capability-atom-shard-map.html#pipeline.slice.operational-execution`.

The manifest step produces `result.md`, gates on `result_truth_recorded`, fails by `stop_or_handoff`, and reaches `ready_for_next_step` only when the result preserves live/current proof boundaries. The operational execution contract requires explicit authority, preflight, stop-condition evaluation, action ledger and execution log when action occurred, post-action validation proof, recovery notes, highest validated truth, and no forbidden claim such as completion without post-action proof.

## Operating Procedure
1. Load the closure packet: parent Slice identity, selected operational execution variant, operation intent, authority-confirmation, current-state baseline, preflight, risk-stop-conditions, action-ledger, execution-log, checkpoint results, post-action-validation, observation-window, rollback, recovery-notes, handoff notes, deferred items, and explicit proof gaps.
2. Separate actions from proof before drafting. Treat commands/tool calls, deploys, restarts, chain transactions, DB writes, config changes, rollback attempts, manual actions, `action-ledger.md`, and `execution-log.md` as action evidence only. Treat checkpoint verification, post-action validation, live endpoint checks, logs, DB/API/chain receipts, UI confirmation, version checks, monitoring observations, and user-returned proof as validation evidence only when each has timestamp, source, target, actor, authority basis, and freshness.
3. Build the operational proof ladder. Use these proof ceilings without upgrading between them:
   - preflight proof says the operation was ready to try, not that the target changed;
   - action proof says the operation was attempted, not that it succeeded live;
   - checkpoint proof says an intermediate expectation held, not that final validation passed;
   - post-action validation can support executed-with-declared-proof-level only for the target and freshness it actually covers;
   - observation proof can support current stability only for the watched signal and window;
   - rollback or recovery proof can support `rolled_back_verified` or recovery posture only when post-rollback state was verified;
   - missing, stale, wrong-target, or authority-limited proof caps the result at blocked, partial, or handoff-ready.
4. Classify the highest validated truth from execution-log, checkpoint verification, post-action validation, observation, recovery notes, authority, proof freshness, and missing proof. Prefer the strongest state that current evidence supports: live/current verified execution, partial execution with missing proof, stopped by stop condition, blocked by missing authority, blocked by missing preflight, blocked by missing post-action proof, rolled back verified, recovery pending, observation pending, or handoff ready. Downgrade any desired completion claim when proof is stale, absent, from the wrong target, authority-limited, or only implied by local/preflight/action evidence.
5. Draft `result.md` with terminal state, `highest_validated_truth`, action summary, proof basis, proof freshness, authority basis, target/environment, what changed, what was verified live/current, what is unverified, missing proof, forbidden claims, stop-condition state, rollback/recovery/observation state, residual risk, deferred work, handoff owner, exact next action, and promotion or maintenance route when separate follow-up is appropriate. When the recorded business outcome is explicitly partial and the same Operational Execution Slice must remain resumable, add an `Outcome` field whose first sentence is exactly `Outcome: partial.`; a same-line explanatory sentence may follow. Runtime uses that field only to retain navigation at the manifest's safe resume gate; it does not grant authority, approve execution, or make prior proof fresh.
6. Block unsupported claims. If the requested result says complete, live, safe, recovered, rolled back, promoted, cleaned up, or no-risk without matching proof, write the result as blocked, partial, rolled back only if verified, or handoff-ready. Name the missing artifact, command output, live check, observation window, authority, freshness, or user evidence needed to upgrade the truth.
7. Close or hand off. Advance to `ready_for_next_step` only after `result_truth_recorded` is true in `result.md`. Route durable learning to `slice-op-exec-promotion-router`; route open monitoring, manual validation, missing authority, context transfer, or recovery follow-up to maintenance or handoff; route failed live behavior back to rollback/recovery only when authority exists.

## Outputs
The required output is `result.md`. It must include terminal state, `highest_validated_truth`, action/proof separation, evidence links or source paths, live/current proof class, proof freshness, authority basis, missing proof, forbidden claims, stop-condition result, rollback or recovery posture, observation status, residual risk, deferred work, handoff requirements, and next route. A resumable partial outcome must use the exact `Outcome: partial.` first sentence; prose mentions and prefixed values do not reopen the pipeline.

Allowed terminal results include `executed_with_declared_proof_level`, `rolled_back_verified`, `stopped_by_stop_condition`, `blocked_missing_authority`, `blocked_missing_preflight`, `blocked_missing_post_action_proof`, and `handoff_ready`. The result may say action attempted, checkpoint passed, validation missing, observation pending, recovery pending, or proof stale, but it must not upgrade local, preflight, checkpoint, or action evidence into live proof. This skill may describe promotion candidates or maintenance needs, but it does not perform promotion, durable-domain writes, operational commands, rollback, recovery, live validation, deployment, source mutation, branch cleanup, or maintenance repair.

## Verification
Trigger verification must select this skill only when an operational execution Slice is at result closure and has enough prior artifacts to classify the outcome or honestly block it. It must reject scenarios still confirming authority, running preflight, executing actions, validating post-action proof, observing a window, performing rollback/recovery, writing recovery notes, routing promotion, or doing maintenance.

Content verification checks that `result.md` records the highest validated truth, separates action from proof, preserves live/current proof freshness, names missing proof, blocks forbidden claims, records blocked states, captures rollback/recovery/observation state, names deferred work and next owner, and never claims completion merely because an action was attempted. When `Outcome: partial` is present, verify that it is truthful and that the Result remains unchanged while handoff navigation advances through fresh gates; never delete or rewrite partial truth merely to move the cursor.

Run `node tools/validate-internal-skill-body-quality.mjs --skill slice-op-exec-result-writer` and `node tools/validate-internal-skill-trigger-fixtures.mjs --skill slice-op-exec-result-writer`.

## Failure Modes
Stop or hand off when closure inputs are missing, authority is absent, preflight status is unknown, stop conditions were triggered without disposition, the action ledger or execution log is missing for performed action, post-action validation is absent or stale, live proof points at the wrong target, rollback authority is missing, rollback happened but was not verified, recovery notes are absent, observation remains pending, or the requested terminal claim overstates the evidence.

Block with `blocked_missing_post_action_proof` when an operation ran but current proof is missing. Use `blocked_missing_authority` or `blocked_missing_preflight` when those gates never passed. Use `stopped_by_stop_condition` when the stop rule owns the terminal truth. Use `rolled_back_verified` only when rollback proof exists. Use `handoff_ready` when the next proof, recovery, observation, or authority action belongs to the user or another owner.

Use zero-upgrade language until missing proof, authority, freshness, recovery, rollback, or observation evidence exists.
