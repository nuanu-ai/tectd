---
id: "slice-lightweight-verification-runner"
kind: "internal_step"
owner_family: "slice_variant"
pipeline_id: "slice.lightweight-tdd-development"
step_id: "slice-lightweight-verification-runner"
entry_gate: false
public_trigger: false
native_skill_discovery: false
body_path: "capabilities/instructions/pipelines/slice-variants/lightweight-tdd/slice-lightweight-verification-runner.step.md"
source_manifest: "capabilities/pipelines/slice-variants/lightweight-tdd.pipeline.json"
legacy_skill_ref: "slice-lightweight-verification-runner"
architecture_refs:
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#lightweight-and-debug-slice-variants"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19"
---

# Slice Lightweight Verification Runner

## Overview

This skill verifies a lightweight TDD Slice after implementation. The core rule is evidence before claim: lightweight means lower ceremony, not lower proof, and this step may only create local verification truth.

It adapts `superpowers:verification-before-completion` into an Tect-owned local proof gate. It produces `verification.md`, records the highest local truth, and blocks when focused or affected proof is missing, stale, failing, or too narrow.

## When to Use

Use this when Runtime has selected `slice.lightweight-tdd-development`, the TDD loop has produced `tdd-notes.md` or `test-plan.md`, `implementation-notes.md` exists, and the next needed artifact is `verification.md`.

Use it for a small code, config, documentation, schema, fixture, or business-rule change that needs fresh focused proof, fresh affected proof, build/lint/typecheck proof, or local runtime proof when the Slice contract requires it. Also use it when an agent is about to claim done, fixed, safe, complete, ready, or locally verified from a single passing command without checking the declared affected surface.

Do not use it to choose tests, execute implementation, repair failures, change source, deploy, validate live systems, write `result.md`, route promotion, close the Slice, or mutate durable knowledge. Escalate to full, debug, hybrid, operational, or user/team handoff when ambiguity, repeated failure, missing testability, deploy risk, live risk, or missing authority exceeds the lightweight contract.

## Source Contract

- Manifest step: `capabilities/pipelines/slice-variants/lightweight-tdd.pipeline.json#step_graph.steps.slice-lightweight-verification-runner`.
- Architecture anchors: `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#lightweight-and-debug-slice-variants`, `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6`, `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19`, `docs/architecture/master-plugin-target-architecture-part-5-pipeline-fabric.html#13-gates-proof-and-terminal-states`, and `docs/architecture/master-plugin-target-architecture-validation-harness-evals.html#6-behavior-scenario-evals`.
- Atom row: `docs/architecture/master-plugin-target-architecture-capability-atom-shard-map.html#pipeline.slice.lightweight_tdd.verification.runner`.
- Reference adapted: `skills/references/superpowers/verification-before-completion/SKILL.md`; use proof-before-claim discipline, fresh command evidence, and explicit forbidden claims, but keep the behavior scoped to lightweight local proof.

The manifest declares this as the validator step that produces `verification.md`, gates on `focused_and_affected_proof`, and exits only as `completed_local_verified` or `blocked_missing_proof`.

## Operating Procedure

1. Load the local Slice proof contract: `slice.md`, acceptance checks, `tdd-notes.md` or `test-plan.md`, `implementation-notes.md`, changed files, affected surfaces, declared commands, authority limits, and any deploy/live proof requirement. If target behavior, affected surface, allowed commands, or required proof is unclear, stop with `blocked_missing_proof`.
2. Freeze the verification boundary. This step may run only local verification commands already authorized by the Slice contract or inspect fresh local evidence. It must not edit implementation, change tests to pass, deploy, call live services, write `result.md`, write `promotion.md`, or close the Slice.
3. Build a local proof matrix before running commands. It must name focused proof, affected proof, static/build proof, local runtime proof when required, RED/GREEN integrity proof, skipped checks, and the claim each check can support. No command may be used as evidence for a claim it does not prove.
4. Run focused proof first. Use the smallest test, acceptance command, fixture validation, or local observation that proves the changed behavior and would have failed before the TDD change. Record command, working directory, timestamp or recency, exit code, and the relevant output summary.
5. Run affected proof second. Cover neighboring tests, integration slices, build, lint, typecheck, package validation, documentation validation, schema validation, or local runtime checks declared by the Slice. If no meaningful affected command exists, record why and block unless the proof contract explicitly accepts substitute evidence.
6. Audit TDD integrity. `verification.md` must state whether RED was observed, why the failure was meaningful, whether GREEN passed, and whether final proof still exercises the intended behavior. If RED was never observed, the test passed immediately, the final proof is mock-only, or the check no longer covers the behavior, route backward instead of claiming verification.
7. Separate proof classes in a deploy/live proof ledger. Label deploy proof, live proof, user/team handoff, rollback proof, and promotion proof as pending, unavailable, delegated, or not required by the Slice contract. This skill may record those needs but must not execute them.
8. Choose the terminal state and next route. Use `completed_local_verified` only when fresh focused and affected local proof covers the declared change and all required local checks are satisfied. Use `blocked_missing_proof` for any missing, stale, failing, contradictory, unavailable, unauthorized, or wrong-scope proof.
9. Write `verification.md` only. Record nonempty fields `focused_command:`, `focused_exit_code:`, `focused_assertion:`, `affected_command:`, `affected_exit_code:`, `affected_assertion:`, and `proof_target_binding:`; when a proof is blocked or not applicable, its field must contain the exact blocker or approved disposition rather than remain empty. After those dispositions are truthful, add exact attestations `focused_proof_disposition_recorded: true`, `affected_proof_disposition_recorded: true`, `command_evidence_or_blocker_recorded: true`, `proof_target_binding_or_gap_recorded: true`, and `verification_receipt_complete: true`. Always include the exact line `gate: focused_and_affected_proof` plus exactly one exact terminal line: `terminal_state: completed_local_verified` or `terminal_state: blocked_missing_proof`. Include the local proof matrix, deploy/live proof ledger, forbidden completion claims, missing proof, residual risk, and next route. A preflight-only, placeholder, two-line success claim, empty-field receipt, or `not_run` document must never contain the completion attestation or gate line. Then hand off to `slice-deploy-impact-checker`, `slice-lightweight-result-writer`, `slice-lightweight-promotion-router`, `slice-lightweight-maintenance-and-handoff`, or an escalation path as the recorded state requires.

## Outputs

Produce exactly one primary artifact: `verification.md` for the selected lightweight Slice.

Required shape:

- Slice id and selected variant.
- Target behavior and acceptance checks.
- Changed files or implementation evidence source.
- Local proof matrix: focused proof, affected proof, build/lint/typecheck/static proof, local runtime proof when required, RED/GREEN integrity, command evidence, freshness, and exit codes.
- Deploy/live proof ledger: deploy proof, live proof, user/team handoff, rollback proof, and promotion proof separated from local proof.
- Skipped, unavailable, stale, failed, or wrong-scope checks.
- Missing proof and residual risk.
- Forbidden completion claims that are not supported.
- Terminal state: `completed_local_verified` or `blocked_missing_proof`.
- Exact evidence fields and attestations from step 9, `gate: focused_and_affected_proof`, and exactly one matching `terminal_state:` line.
- Next route.

`completed_local_verified` means local verification only. It is not deployed, live, promoted, closed, safe-to-merge, or result-complete unless a later step records matching proof. `blocked_missing_proof` means the Slice cannot advance through result or promotion as verified until the missing proof is produced, explicitly waived as authority input, or routed to an escalation.

## Verification

Before leaving this skill:

- `verification.md` exists and names the exact focused and affected proof.
- Every local claim has fresh proof or is explicitly marked stale, skipped, failed, unavailable, delegated, or out of scope.
- The focused proof covers the acceptance behavior, not only an incidental passing command.
- The affected proof covers the declared affected surface or blocks with a named gap.
- RED/GREEN integrity is recorded from `tdd-notes.md`, `test-plan.md`, or command evidence.
- Local, deploy, live, user/team handoff, rollback, result, and promotion proof needs are labeled separately.
- No result, promotion, deployed, live, safe, done, fixed, complete, or ready claim exceeds the recorded proof class.

For skill-body validation, run:
- `node tools/validate-internal-skill-body-quality.mjs --skill slice-lightweight-verification-runner`
- `node tools/validate-internal-skill-trigger-fixtures.mjs --skill slice-lightweight-verification-runner`

## Failure Modes

Block with `blocked_missing_proof` when focused proof was not run, affected proof was skipped, a command failed, evidence is stale, RED/GREEN evidence is missing, proof is mock-only, the target no longer exercises the intended behavior, local runtime proof is required but unavailable, authority is missing, or the Slice cannot explain which surface is affected.

Failure routing:

- Route to `slice-test-target-selector` or `slice-tdd-cycle-runner` when the missing proof is a bad or absent TDD proof target.
- Route to `slice.debug-root-cause` when verification failure reveals unknown cause, contradictory behavior, or repeated failed fixes.
- Route to `slice.full-design-to-execution` when verification exposes architecture ambiguity, cross-component uncertainty, or missing design decisions.
- Route to `slice.hybrid-implementation-operation` when completion depends on deployment, environment mutation, or live proof.
- Route to `slice-deploy-impact-checker` when local proof passes but deploy/live impact still needs classification.
- Route to `slice-lightweight-result-writer` only after `verification.md` records highest local truth and forbidden claims.
- Route to `slice-lightweight-promotion-router` only after result writing has created a promotion candidate or no-promotion decision.
- Route to `slice-lightweight-maintenance-and-handoff` when verification is paused, blocked, delegated to user/team, or needs a durable handoff.

Forbidden completion claims without later proof include: complete, done, fixed, safe, deployed, live, promoted, merged, production-ready, no deploy required, no risk, and result complete. A user waiver can change required next action, but it is not fake evidence.
