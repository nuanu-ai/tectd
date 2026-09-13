---
id: "slice-regression-test-writer"
kind: "internal_step"
owner_family: "slice_variant"
pipeline_id: "slice.debug-root-cause"
step_id: "slice-regression-test-writer"
entry_gate: false
public_trigger: false
native_skill_discovery: false
body_path: "capabilities/instructions/pipelines/slice-variants/debug-root-cause/slice-regression-test-writer.step.md"
source_manifest: "capabilities/pipelines/slice-variants/debug-root-cause.pipeline.json"
legacy_skill_ref: "slice-regression-test-writer"
architecture_refs:
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#debug-root-cause-variant"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19"
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#lightweight-and-debug-slice-variants"
---

# Slice Regression Test Writer

## Overview

This is a reference adapter for `superpowers:test-driven-development` inside Tect's Debug/root-cause Slice variant. Its core rule is: after root cause and fix strategy are known, write or select the smallest real regression proof that should fail before the fix, record it in the step-owned `regression-target.md`, and hand off without executing the source fix.

## When to Use

Use this after `slice-root-cause-decision` and `slice-debug-fix-strategy` have produced `root-cause.md` plus either `fix-plan.md` or `no-fix-result.md`. Select it when the selected `slice.debug-root-cause` path needs gate `regression_proof_target_declared` and terminal state `regression_target_ready` before `slice-debug-fix-runner` may attempt a patch.

Use it for bugs, regressions, broken runtime behavior, or no-fix outcomes where the Slice needs a durable `regression-target.md` naming the exact failing test, command, fixture, assertion, API/runtime probe, log query, or manual proof that constrains the later fix or preserves the negative proof.

Do not use it for initial symptom capture, reproduction discovery, hypothesis work, root-cause decision, fix strategy, ordinary lightweight TDD implementation, broad RED/GREEN/refactor execution, debug patching, deployment validation, live incident recovery, result writing, or promotion. Route those to the surrounding debug, lightweight, operational, hybrid, result, or handoff steps.

## Source Contract

- Record ID: `tect-skill.slice-lightweight-debug.slice-regression-test-writer`
- Pipeline manifest: `capabilities/pipelines/slice-variants/debug-root-cause.pipeline.json`
- Manifest step: `step_graph.steps.slice-regression-test-writer`
- Produces: `regression-target.md`
- Gate: `regression_proof_target_declared`
- On failure: `record_missing_test_target_or_escalate`
- Terminal state: `regression_target_ready`
- Handoff targets: `slice-debug-fix-runner` for authorized patches, `slice-debug-verification-runner` for proof execution, or `slice-debug-handoff-builder` / `slice-debug-result-writer` for blocked and no-fix paths
- Architecture anchors: `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6`, `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19`, `docs/architecture/master-plugin-target-architecture-part-5-pipeline-fabric.html`, `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#lightweight-and-debug-slice-variants`, `docs/architecture/master-plugin-target-architecture-validation-harness-evals.html`, and `docs/architecture/master-plugin-target-architecture-capability-atom-shard-map.html#pipeline.slice.debug_root_cause.regression.test.writer`
- External reference adapted: `skills/references/superpowers/test-driven-development/SKILL.md` plus `skills/references/superpowers/test-driven-development/testing-anti-patterns.md`

Adaptation boundary: this skill imports TDD's RED-before-GREEN discipline and anti-mock guidance, but it is not the lightweight TDD runner. It declares or creates a regression proof target for a debug Slice; it does not perform the cause-level source fix or claim GREEN proof.

## Operating Procedure

1. Enforce the root-cause/fix-plan gate. Read the active Slice packet and require `root-cause.md` plus `fix-plan.md` or `no-fix-result.md`. `root-cause.md` must name the proven cause or explicit no-fix decision; `fix-plan.md` must name the lane, affected surface, downstream authority, and why a regression target is needed. If the cause is still unknown, route back to `slice-root-cause-decision`. If the strategy is missing or vague, route back to `slice-debug-fix-strategy`.
2. Extract the behavior to protect from evidence, not from the imagined patch. Write one sentence with expected behavior, observed failure, root-cause mechanism, affected public/API/runtime/contract surface, and the specific failure mode the future test must catch.
3. Prefer a real behavioral test. First look for an existing failing test or narrow command that already proves the root cause. If absent, identify the smallest new test-only RED target that exercises production behavior through a public API, CLI, integration seam, validator, contract fixture, UI behavior, or runtime probe. Avoid private-method assertions, implementation-detail snapshots, mock-only expectations, and tests that merely prove a mock was called.
4. Decide whether this step may write the RED target. If the active Slice and workspace authority explicitly allow test-only source changes, create only the minimal failing regression test or fixture before any fix. If authority is read-only, unclear, live-system dependent, or outside the current worker boundary, do not edit source; declare the exact test or proof target in `regression-target.md` for the authorized fix/test runner to create or run.
5. Define the RED contract in `regression-target.md`: target kind, file path or command, test name or observable, setup data, assertion, expected failing output, why that failure proves the root cause, and what would invalidate the RED signal. RED must fail before the fix for the known cause, not because of syntax, missing dependencies, stale fixtures, environment drift, or broad suite noise.
6. Define the GREEN boundary separately. State that the same target should pass only after the authorized fix runner addresses the root cause, list focused and affected proof to rerun, and name the later owner. GREEN is a future verification condition for `slice-debug-verification-runner`, not a success claim from this skill.
7. Record fallback proof only when no credible automated RED exists now. Name why automation is unavailable, the substitute reproduction or manual proof, residual risk, the smallest follow-up test task, and whether the current Slice may proceed, must escalate, or must hand off.
8. Run a target-quality review before terminal state: root-cause link present, proof is minimal, behavior is real, mocks are not the subject, RED/GREEN are separated, authority is explicit, no fix execution occurred, and handoff owner is named.
9. End with `regression_target_ready` only when `regression-target.md` lets a later agent run or create the RED proof without inventing behavior. A successful receipt includes `gate: regression_proof_target_declared` and `terminal_state: regression_target_ready`; failure receipts cannot claim that pair. Otherwise stop with `record_missing_test_target_or_escalate`, `blocked_missing_authority`, `blocked_no_reliable_reproduction`, or a named escalation route.

## Outputs

Primary output is the Slice-local `regression-target.md`. It must include: root-cause reference, fix-strategy and selected payload reference, proof target name, target kind, file path or command when known, test name or observable, setup or fixture data, expected RED failure, why the failure proves the root cause, invalid RED conditions, GREEN expectation after the later fix, affected proof to rerun, mock/dependency notes, authority needed before writing or running the proof, fallback proof if any, residual risk, downstream handoff owner, the exact gate, and terminal state.

When authority allows test-only RED creation, the output may also include the created failing test or fixture reference and the focused command expected to fail. It must not include the cause-level source fix, patch execution, deployment proof, live-system mutation, broad TDD cycle notes, promotion records, or final result claims.

## Verification

Content verification checks that `regression-target.md` names a concrete proof target, uses RED before GREEN, ties the target to the root cause rather than the symptom alone, avoids mock-only or implementation-detail tests, records authority before any test-only write/run, and distinguishes a declared proof target from executed GREEN proof. Trigger verification requires at least two positive debug regression-proof scenarios and negative scenarios for initial diagnosis, lightweight TDD, patch execution, or live incident recovery.

Layer 6B validators should pass for this body:

- `node tools/validate-internal-skill-body-quality.mjs --skill slice-regression-test-writer`
- `node tools/validate-internal-skill-trigger-fixtures.mjs --skill slice-regression-test-writer`

For real Slice use, verify the section answers: What exact behavior should fail? Which root cause does it prove? Where is the test/proof target? What command or observable exposes RED? What makes the target invalid? Who owns the later fix and GREEN proof?

## Failure Modes

Block with `record_missing_test_target_or_escalate` when no reliable reproduction exists, the root cause is still unknown, the fix strategy is absent, the target would only test mock behavior, the target only checks implementation internals, the proof needs unapproved test-write/source/live-system authority, or the work has become an operational incident.

Escalate to full design when the target requires cross-component contract decisions or repeated proof-target attempts expose architecture ambiguity. Route to lightweight TDD only when the work is no longer a debug Slice and the cause/acceptance target is already bounded before diagnosis. Route to operational or hybrid pipelines when live recovery, deployment, rollback, or runtime authority dominates. Route to `slice-debug-handoff-builder` when evidence, environment, authority, or target ownership is missing.

## Quick Reference

RED means the pre-fix proof is expected to fail for the known root cause. GREEN means the same proof should pass after the authorized fix. This skill creates or declares the RED target and handoff; `slice-debug-fix-runner` owns the patch, and `slice-debug-verification-runner` owns proof execution and GREEN claims.
