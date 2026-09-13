---
id: "slice-debug-fix-runner"
kind: "internal_step"
owner_family: "slice_variant"
pipeline_id: "slice.debug-root-cause"
step_id: "slice-debug-fix-runner"
entry_gate: false
public_trigger: false
native_skill_discovery: false
body_path: "capabilities/instructions/pipelines/slice-variants/debug-root-cause/slice-debug-fix-runner.step.md"
source_manifest: "capabilities/pipelines/slice-variants/debug-root-cause.pipeline.json"
legacy_skill_ref: "slice-debug-fix-runner"
architecture_refs:
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#debug-root-cause-variant"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19"
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#lightweight-and-debug-slice-variants"
---

# Slice Debug Fix Runner

## Overview
This is a standalone Tect-owned debug fix skill. It adapts the plan review, execute, checkpoint, and stop-when-blocked discipline from `superpowers:executing-plans` as reference material only; that external skill is not a runtime dependency and is not the canonical implementation.

The core rule is exact task execution only: apply or prepare one authorized root-cause fix attempt, record evidence in `patch.md`, and hand off to verification or defer without stacked guesses.

## When to Use
Use this only when the active `slice.debug-root-cause` runtime view is at `slice-debug-fix-runner` and all source inputs are present:

- `symptom.md`, `reproduction.md` or unable-to-reproduce record, `evidence-log.md`, `hypotheses.md`, and `root-cause.md`.
- `fix-strategy.md` plus its selected payload: a bounded `fix-plan.md` for an attempt, or `no-fix-result.md` for a truthful deferred/no-mutation receipt.
- Explicit fix authority covering the intended source mutation or proposed patch when the selected lane attempts a fix. A no-fix lane does not invent mutation authority.
- Attempt history proving this is within the allowed attempt count.

Do not use this for no root-cause discovery, no plan invention, no regression target selection, no verification-only work, no result closure, no promotion, no deployment, no live recovery, and no unrelated source mutation. If those are needed, route to the owning debug, lightweight, full-development, operational, hybrid, result, promotion, or handoff skill.

## Source Contract
Owning manifest: `capabilities/pipelines/slice-variants/debug-root-cause.pipeline.json`, step `slice-debug-fix-runner`. The manifest invokes this skill plus `superpowers:executing-plans`, always produces `patch.md`, gates on `fix_attempt_or_defer_recorded`, fails through `block_or_defer_fix`, and terminates as `fix_attempt_recorded` or `fix_deferred_to_followup`. The stricter `fix_authority_and_root_cause_present` gate applies only to a lane that actually mutates source.

Architecture grounding:
- `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#debug-root-cause-variant`
- `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#lightweight-and-debug-slice-variants`
- `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6`
- `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19`
- `docs/architecture/master-plugin-target-architecture-capability-atom-shard-map.html#pipeline.slice.debug_root_cause.fix.runner`

Relevant constraints: debug requires root cause before fix; three failed attempts route to architecture discussion or follow-up Slice; Slice parent owns target, selected variant, artifact contract, proof contract, and result boundary; variant manifests own internal steps and terminal states.

## Operating Procedure
1. Read `fix-strategy.md`, its one selected payload, `regression-target.md`, and the root-cause source inputs. Reject coexistence or absence of both branch payloads, mismatched lane/terminal state, or a root cause that is missing, contradicted, symptom-only, or not tied to evidence.
2. Resolve the branch before plan review. For `no-fix-result.md`, record a truthful no-mutation defer in `patch.md`; name the missing authority/evidence or escalation target, preserve the regression/substitute proof target, and do not require or fabricate a `fix-plan.md`. For `fix-plan.md`, continue through the fix-attempt checks below.
3. Confirm the fix-lane entry gate `fix_authority_and_root_cause_present`. Review the fix plan as a checkpoint plan. It must state the root cause addressed, the exact task, target files or components, proof order, expected checkpoint verification after each task, rollback or defer conditions, and stop conditions. If the plan is ambiguous or tries to change the diagnosis, defer instead of rewriting it.
4. Check authority and boundary. Mutation is allowed only inside the active Slice contract and only for the named files, commands, or proposed patch. Missing authority, dirty-target uncertainty, live-system authority, deployment authority, or cross-component redesign means `fix_deferred_to_followup`.
5. Set the attempt ledger before mutation. Record attempt number, prior attempts, selected task, expected evidence, allowed files, forbidden files, expected verification command or evidence, rollback note, and immediate stop condition.
6. Execute only the exact task authorized by the plan. Do not expand scope, add opportunistic refactors, repair adjacent problems, change tests after they pass, or make a second guessed patch when the first patch fails.
7. Run checkpoint verification after each task when authority and environment allow it. If a command cannot run, record the unavailable command, reason, and substitute evidence or defer condition. If verification fails and the plan does not name the next bounded recovery action, stop.
8. Preserve the common receipt. `patch.md` must contain `gate: fix_attempt_or_defer_recorded` and exactly one matching terminal state. Capture changed files or explicit no-mutation truth, command output or evidence references, deviations from plan, failed/skipped proof, rollback/defer note, and residual risk.
9. Stop after the attempt or defer. Do not continue into result writing, promotion, deployment, live validation, or another patch. A patch that compiles locally is still not completion.
10. Route the next owner. Send `patch.md` to `slice-debug-verification-runner` when proof can continue. Send a deferred or blocked attempt to `slice-debug-handoff-builder` or back to `slice-debug-fix-strategy`, `slice-root-cause-decision`, full-development, operational, hybrid, or architecture discussion as the recorded gap requires.

## Outputs
Produce `patch.md` when a bounded attempt was made, prepared, or deferred at this step. It must include:

- Root-cause reference, fix-strategy reference, selected payload reference, `gate: fix_attempt_or_defer_recorded`, and terminal state.
- Attempt number, attempt limit, and whether three failed attempts have been reached.
- Exact task, changed files or proposed patch, and files explicitly left untouched.
- Root cause addressed, non-goals, and why this is not a symptom patch.
- Checkpoint plan, command output or evidence, failed/skipped proof, and verification status.
- Rollback note, defer note, handoff owner, residual risk, and forbidden claims.

Use `terminal_state: fix_attempt_recorded` only when the authorized attempt was recorded with evidence and is ready for debug verification. Use `terminal_state: fix_deferred_to_followup` for a selected no-fix lane or when root cause, authority, bounded plan, proof order, environment, rollback, or safety conditions block the attempt. Both require the common gate marker; neither permits an absent `patch.md`.

`patch.md` is not completion, not `verification.md`, not `result.md`, and cannot support false completion from patch alone.

## Verification
Trigger verification requires scenarios where root cause, fix authority, bounded plan, proof order, rollback or defer conditions, and exact attempt scope are already known. Non-trigger scenarios must reject diagnosis, strategy selection, verification, deployment, live recovery, promotion, result writing, and unauthorized mutation.

Content verification checks that this body names the manifest path, architecture HTML anchors, `superpowers:executing-plans` as reference material only, entry gate, source inputs, plan checkpoint review, exact task execution only, checkpoint verification, `patch.md` artifact shape, terminal states, handoff routes, and forbidden action boundaries.

Run:
- `node tools/validate-internal-skill-body-quality.mjs --skill slice-debug-fix-runner`
- `node tools/validate-internal-skill-trigger-fixtures.mjs --skill slice-debug-fix-runner`

During real use, verify every patch claim against fresh checkpoint evidence before advancing. If proof is missing, record the missing proof and route instead of upgrading the state.

## Failure Modes
Block or defer with `fix_deferred_to_followup` when root cause is absent, stale, contradicted, or symptom-only; when authority does not cover the target files or commands; when the fix plan lacks a bounded task, checkpoint, proof order, rollback, or defer condition; when environment access is missing; when verification fails without a named recovery action; or when three failed attempts have already happened.

Stop immediately on stacked guesses, plan invention, self-authorized source mutation, unrelated source mutation, hidden redesign, live-system action, deployment authorization, result closure, promotion decision, or any request to treat `patch.md` as proof of completion.

Route backward to root-cause decision or fix strategy when the diagnosis or plan is incomplete. Route sideways to operational execution or hybrid implementation when the safe repair is rollback, deploy, data repair, live recovery, or code plus live proof. Route forward only to `slice-debug-verification-runner` after `patch.md` records the authorized attempt.

## Handoff Rules
The handoff must name the next owner, terminal state, exact reason, and evidence path. Use `slice-debug-verification-runner` for recorded attempts, `slice-debug-handoff-builder` for missing access or human action, `slice-debug-fix-strategy` for plan gaps, `slice-root-cause-decision` for diagnosis gaps, and full-development or architecture discussion when repeated failures or design uncertainty exceed the debug Slice.
