---
id: "slice-debug-verification-runner"
kind: "internal_step"
owner_family: "slice_variant"
pipeline_id: "slice.debug-root-cause"
step_id: "slice-debug-verification-runner"
entry_gate: false
public_trigger: false
native_skill_discovery: false
body_path: "capabilities/instructions/pipelines/slice-variants/debug-root-cause/slice-debug-verification-runner.step.md"
source_manifest: "capabilities/pipelines/slice-variants/debug-root-cause.pipeline.json"
legacy_skill_ref: "slice-debug-verification-runner"
architecture_refs:
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#lightweight-and-debug-slice-variants"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19"
---

# Slice Debug Verification Runner

## Overview
This is Tect's debug verification skill and a reference adapter for `skills/references/superpowers/verification-before-completion/SKILL.md`. Its core rule is evidence before claims: no reproduced, unreproduced, root-cause, fixed, no-fix, regression, deployment, live-state, or complete claim is allowed unless fresh proof covers that exact claim and proof class.

The skill does not diagnose or repair the bug. It verifies the current debug Slice truth, writes the verification artifact, and blocks or routes backward when proof cannot support the next state.

## When to Use
Use this when the active work is `slice.debug-root-cause` and the manifest step is the verification gate that produces `verification.md`. Typical trigger points are after:

- `symptom.md` and `reproduction.md` or an explicit unable-to-reproduce record exist.
- `root-cause.md` states a proven cause, an unknown-cause blocker, or a no-fix decision.
- `fix-strategy.md`, its selected `fix-plan.md` or `no-fix-result.md` payload, `regression-target.md`, and `patch.md` have been produced.
- The agent is about to claim fixed, unable to reproduce, no fix needed, root cause found, regression protected, locally verified, deployed, live verified, or ready for result writing.

Do not use this for first diagnosis, evidence ordering, root-cause declaration, regression target authoring, fix strategy, patch execution, deployment, live incident response, promotion, or final result writing. Route backward when proof is missing, stale, contradictory, does not reproduce under the declared conditions, or does not cover the declared root cause. Route to deployment/live validation only when a separate authority gate explicitly makes deployment or live proof part of the claim.

## Source Contract
Grounding sources:
- `capabilities/pipelines/slice-variants/debug-root-cause.pipeline.json` step `slice-debug-verification-runner`
- `capabilities/pipelines/slice-variants/debug-root-cause.pipeline.json#step_graph.steps.slice-debug-verification-runner.invokes.slice-debug-verification-runner`
- `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#lightweight-and-debug-slice-variants`
- `docs/architecture/master-plugin-target-architecture-part-5-pipeline-fabric.html`
- `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6`
- `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s16`
- `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19`
- `docs/architecture/master-plugin-target-architecture-capability-atom-shard-map.html#pipeline.slice.debug_root_cause.verification.runner`
- `docs/architecture/master-plugin-target-architecture-validation-harness-evals.html`
- `skills/references/superpowers/verification-before-completion/SKILL.md`

The manifest step is a validator step that invokes this skill plus `superpowers:verification-before-completion`, produces `verification.md`, gates on `reproduction_or_no_repro_truth_verified` and `regression_proof_recorded`, fails with `block_missing_proof`, and ends in `root_cause_found` or `blocked_missing_proof`.

## Operating Procedure
1. Load the verification packet. Read `slice.md`, `symptom.md`, `reproduction.md` or the unable-to-reproduce record, `evidence-log.md`, `hypotheses.md`, `root-cause.md`, `fix-strategy.md`, its selected `fix-plan.md` or `no-fix-result.md`, `regression-target.md`, and `patch.md`. If a required input is missing, name the missing artifact before choosing proof.
2. Name the exact claim under verification. Classify it as reproduced, unable to reproduce, root cause found, root cause unknown, no-fix, fixed locally, regression protected, deploy verified, live verified, or blocked. Do not select a proof command until the claim and proof class are explicit.
3. Verify reproduction or no-repro truth first. Rerun or freshly inspect the declared reproduction path, command, log query, trace, runtime state, API call, screenshot, or comparison evidence. If the symptom no longer reproduces, require an explicit no-repro record with environment, timestamp, commands, and limits; absence of that record blocks the Slice.
4. Verify root-cause coverage. Check that at least one proof directly exercises the named cause or the data/control/state path that failed. A passing UI check, snapshot, or broad test is insufficient if it can pass while the diagnosed cause remains untested.
5. Verify RED/GREEN or discriminating regression truth. When a regression test or probe is feasible, record the RED side against the broken behavior or a documented pre-fix failure, then record the fresh GREEN side after the fix or no-fix decision. If RED cannot be rerun safely, record the reason and substitute the strongest discriminating evidence instead of pretending a red-green cycle happened.
6. Verify fix, no-fix, or deferred truth. For a patch, prove the original symptom and the root-cause path changed in the intended way. For no-fix or deferred outcomes, prove why the honest state is obsolete report, environment issue, user misunderstanding, missing authority, missing evidence, follow-up Slice, or unresolved root cause.
7. Verify focused and affected proof. Run or inspect the focused regression target plus the affected test group, build, lint, typecheck, runtime check, or manual proof declared by the Slice. If the affected surface cannot be tested, record the unavailable command, data, service, device, credential, environment, or authority.
8. Separate local, deploy, and live proof. Label every proof item as local, deploy, live, user-handoff, or unavailable. Local tests cannot support deployed or live claims. Deployment metadata cannot support user-visible behavior unless a fresh live/API/runtime proof exists. Live-system commands require separate authority; this skill only records live proof when such authority already exists.
9. Handle failed verification without guessing. If a proof command fails, times out, is flaky, contradicts the root-cause claim, or exposes a new symptom, stop the success path. Record the observed failure, preserve the exact output or evidence reference, and route to reproduction, evidence ordering, root-cause decision, fix strategy, regression target selection, or handoff. Do not stack another unplanned fix from this skill.
10. Write `verification.md`. Include the claim, source inputs, commands or evidence references, outputs, timestamps or freshness notes, proof class, RED/GREEN or substitute evidence, reproduction/no-repro truth, root-cause coverage, focused proof, affected proof, local/deploy/live boundary, failed checks, forbidden claims, `gate: debug_verification_recorded`, terminal state, and next route.
11. Gate the handoff. Route forward to `slice-debug-result-writer` only when `verification.md` honestly proves the manifest gates for the declared proof class. Otherwise end with `blocked_missing_proof` and route to the earliest missing-proof owner.
12. Preserve side-effect boundaries. This skill may run or inspect verification commands when authority already allows them and may write `verification.md`; it does not authorize source mutation, deployment, promotion, merge, push, branch operations, durable-domain writes, maintenance repair, or live-system operation.

## Outputs
Produce exactly one debug verification artifact for this step: `verification.md`.

The artifact must include:

- Verification target: claim, proof class, source inputs, and expected terminal state.
- Reproduction truth: reproduced, unable to reproduce, or blocked with exact conditions.
- Root-cause coverage: which proof exercises the named cause, not only the symptom.
- Regression proof: RED/GREEN truth or documented substitute evidence.
- Focused and affected proof: commands, outputs, skipped checks, and why any substitute is acceptable.
- Local/deploy/live boundary: highest proof class and explicit forbidden claims.
- Failed verification handling: failed command output, contradictory evidence, stale proof, unavailable environment, or missing authority.
- Next route: `slice-debug-result-writer` only after proof, or the backward/handoff owner when blocked.

Use `terminal_state: root_cause_found` only when fresh evidence proves reproduction or no-repro truth, root-cause coverage, regression proof, and fixed/no-fix behavior at the declared proof class. Use `terminal_state: blocked_missing_proof` when evidence is missing, stale, contradictory, does not reproduce under expected conditions, skips the root-cause path, omits affected proof, lacks required authority, or would overclaim local proof as deploy/live proof. Both are explicit verification-stage outcomes paired with `gate: debug_verification_recorded`; a Step-13 target declaration cannot satisfy this final verification receipt.

## Verification
Trigger verification requires positive scenarios where a debug Slice is at its manifest verification gate after reproduction, root-cause, regression target, fix, no-fix, or failed proof work. Negative scenarios are diagnosis, patching, deployment execution, promotion, or result writing before `verification.md` exists.

Content verification requires the body to preserve the manifest path, architecture HTML sources, `superpowers:verification-before-completion` markers, source inputs, exact proof gates, terminal states, failed verification handling, local/deploy/live separation, and the result-writer handoff gate. During real use, the agent must identify the proof for each claim, run or inspect it freshly when authority allows, read the result, and record gaps instead of upgrading the claim.

Run:
- `node tools/validate-internal-skill-body-quality.mjs --skill slice-debug-verification-runner`
- `node tools/validate-internal-skill-trigger-fixtures.mjs --skill slice-debug-verification-runner`

## Failure Modes
Block with `blocked_missing_proof` when reproduction truth is absent, no-repro evidence is thin, proof is stale, the symptom cannot be reproduced under declared conditions, root-cause proof is symptom-only, RED/GREEN or substitute regression proof is missing, focused proof fails, affected tests are skipped without a substitute, runtime proof cannot run, or proof does not cover the named root cause.

Block when the verification request would require source mutation, deployment, live-system commands, promotion, branch operations, durable-domain writes, or maintenance repair not separately authorized. Block false completion when local proof is being used as live proof, when deployment status is inferred from tests, when user-facing behavior is unverified, when a previous run is reused without freshness justification, or when a failed verification is followed by another guessed fix.

Route backward to reproduction, evidence ordering, recent-change inspection, working-example comparison, root-cause decision, regression-test writing, fix strategy, or fix runner when proof gaps are diagnostic. Route sideways to operational/hybrid/deployment validation when live or deploy authority dominates the claim. Route to handoff when missing environment, data, authority, or human action blocks proof.

Route forward to result writing only after `verification.md` states the highest proof class honestly and preserves every missing or forbidden claim. If the user asks for completion while proof is blocked, report the blocker and handoff route; do not soften the terminal state.
