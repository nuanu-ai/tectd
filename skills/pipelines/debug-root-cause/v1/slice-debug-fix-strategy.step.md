---
id: "slice-debug-fix-strategy"
kind: "internal_step"
owner_family: "slice_variant"
pipeline_id: "slice.debug-root-cause"
step_id: "slice-debug-fix-strategy"
entry_gate: false
public_trigger: false
native_skill_discovery: false
body_path: "capabilities/instructions/pipelines/slice-variants/debug-root-cause/slice-debug-fix-strategy.step.md"
source_manifest: "capabilities/pipelines/slice-variants/debug-root-cause.pipeline.json"
legacy_skill_ref: "slice-debug-fix-strategy"
architecture_refs:
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#debug-root-cause-variant"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19"
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#lightweight-and-debug-slice-variants"
---

# Slice Debug Fix Strategy

## Overview
This reference-adapter skill converts `superpowers:writing-plans` discipline into the post-root-cause strategy gate for `slice.debug-root-cause`. The core rule is: root-cause proof selects the strategy, and this skill may produce a plan or defer record but never a patch, deployment, live command, or source mutation.

## When to Use
Use this after `symptom.md`, `reproduction.md` or an unable-to-reproduce record, `evidence-log.md`, `hypotheses.md`, and `root-cause.md` exist. `root-cause.md` must name the proven cause, evidence path, rejected hypotheses, confidence, and why a symptom-level patch would be insufficient.

Do not use this for initial diagnosis, evidence ordering, hypothesis work, root-cause declaration, regression-test writing, fix execution, verification, result writing, live incident response, or broad design work. If the requested change is already a small known task before debug evidence exists, route to `slice.lightweight-tdd-development`.

## Source Contract
The owning manifest is `capabilities/pipelines/slice-variants/debug-root-cause.pipeline.json`. The manifest step is `slice-debug-fix-strategy`; it invokes this skill plus `superpowers:writing-plans`, produces the branch-neutral `fix-strategy.md` receipt plus exactly one payload, `fix-plan.md` or `no-fix-result.md`, is gated by `fix_strategy_after_root_cause`, fails through `defer_fix_or_block`, and ends as `fix_plan_ready` or `fix_deferred_to_followup`.

Architecture grounding: `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#debug-root-cause-variant`, `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#lightweight-and-debug-slice-variants`, `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6`, `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19`, `docs/architecture/master-plugin-target-architecture-capability-atom-shard-map.html#pipeline.slice.debug_root_cause.fix.strategy`, `docs/architecture/master-plugin-target-architecture-part-5-pipeline-fabric.html`, and `docs/architecture/master-plugin-target-architecture-validation-harness-evals.html`.

Reference coverage: adapt `superpowers:writing-plans` into a bounded implementation plan shape with concrete task boundaries, exact verification targets, no placeholders, and a self-review pass. The external skill is source material only; do not copy it or treat it as the canonical Tect implementation.

## Operating Procedure
1. Enforce the proven root-cause gate. Read `root-cause.md` and confirm it states the cause, evidence, rejected guesses, confidence, affected surface, and why symptom patching is wrong. If any part is missing, stop without `fix-plan.md` and route back to `slice-root-cause-decision` or write `no-fix-result.md` with terminal state `fix_deferred_to_followup`.
2. Choose exactly one lane from the proven cause:
   - `no-fix`: a patch is not justified because evidence, authority, rollback, or proof is missing.
   - `minimal-fix`: one bounded code, config, or data-shape change addresses the cause inside the current Slice.
   - `tdd-fix`: a regression proof target can be written first, then the fix can be run by later skills.
   - `full-development-escalation`: the cause requires cross-component design, contract changes, or broader implementation planning.
   - `operational-escalation`: the next safe action is live recovery, deploy, data repair, rollback, or another operational command.
   - `hybrid-escalation`: code change and live operation are inseparable and require both local and live proof.
   - `architecture-discussion`: the invariant, ownership, or desired behavior is unresolved, or three fix attempts have already failed.
3. Produce `fix-plan.md` only for `minimal-fix` or `tdd-fix` when the current Slice can safely continue. The plan must include the root-cause reference, chosen lane, exact affected files or components if known, one primary task sequence, explicit non-goals, authority needed by later skills, regression proof target, verification commands or evidence, rollback or defer criteria, and stop conditions.
4. Adapt writing-plans mechanics without starting execution. Break the strategy into bite-sized tasks such as "write or select regression proof", "verify the proof fails or reproduces", "make the minimal cause-level change", and "run declared verification". Each task must have expected evidence. Do not include placeholders like "add appropriate tests" or "handle edge cases"; name the concrete target or record why it is unknown.
5. Produce `no-fix-result.md` for `no-fix`, full-development, operational, hybrid, or architecture-discussion lanes. Record why the current debug Slice must not patch, which evidence or authority is missing, the follow-up Slice or handoff target, residual risk, rollback/defer posture, and terminal state `fix_deferred_to_followup`.
6. Run a strategy self-review before handoff: cause coverage, task-boundary fit, placeholder scan, file/component consistency, verification/proof target, rollback/defer criteria, authority needs, and forbidden-overreach scan. If the review fails, revise the payload or switch to `no-fix-result.md`.
7. Write `fix-strategy.md` after the payload is settled. Record `strategy_lane`, `strategy_artifact_ref`, `strategy_artifact_kind`, `gate: fix_strategy_after_root_cause`, and exactly one terminal state: `fix_plan_ready` or `fix_deferred_to_followup`. The common receipt must point to the one payload that actually exists and must never invent a second branch artifact.
8. Route explicitly. Send the strategy receipt and selected payload to `slice-regression-test-writer`; a no-fix branch still records the smallest regression/substitute proof target and a truthful deferred fix receipt before verification/result handoff. Escalate to the full-development, operational-execution, hybrid, or architecture route named by the lane.

## Outputs
`fix-strategy.md` is the branch-neutral completion receipt. It names the selected lane, the exact payload reference/kind, `gate: fix_strategy_after_root_cause`, and the matching terminal state so continuation never has to guess which branch file exists.

`fix-plan.md` is a bounded implementation plan, not a patch and not completion. It must contain: proven cause, selected lane, affected surface, exact task boundaries, non-goals, authority needs, regression proof target, expected verification, rollback/defer criteria, stop conditions, downstream owner, and terminal state `fix_plan_ready`.

`no-fix-result.md` is the proof-backed decision not to patch in this debug Slice. It must contain: root-cause reference or missing-proof reason, selected no-fix/escalation lane, evidence or authority gap, follow-up route, residual risk, handoff needs, and terminal state `fix_deferred_to_followup`.

## Verification
Verify that `fix-strategy.md` exists and exactly one payload exists: `fix-plan.md` or `no-fix-result.md`. The receipt and payload must reference `root-cause.md`, agree on the selected lane and terminal state, and reject coexistence of both payloads. A `fix-plan.md` must include at least one concrete task, authority needs, regression proof target, verification target, rollback/defer criteria, and stop conditions. A `no-fix-result.md` must explain why mutation is unsafe and name a follow-up or handoff route.

Check forbidden claims line by line: no fix before root-cause proof, no stacked guesses, no symptom patch as root cause, no source mutation authorization from this skill, no deployment or live-system command, no broad redesign hidden inside a debug fix plan, and no result/completion claim.

## Failure Modes
Stop with `fix_deferred_to_followup` when root-cause proof is missing, confidence is too low, the affected surface cannot be bounded, authority is absent, rollback cannot be named, the regression proof target is unclear, or the needed work belongs to full development, operational execution, hybrid implementation plus operation, or architecture discussion.

If repeated fix attempts already failed, do not create another guess-driven plan. Route to architecture discussion or full-development follow-up. This skill has no execution, source mutation, branch/worktree mutation, deployment, live-system, verification-running, or result-writing authority.

## Core Pattern
Proven root cause chooses one lane; the lane chooses either a bounded `fix-plan.md` or a documented `no-fix-result.md`; later skills own test writing, mutation, verification, result, promotion, or handoff.
