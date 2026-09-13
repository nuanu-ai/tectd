---
id: "slice-tdd-cycle-runner"
kind: "internal_step"
owner_family: "slice_variant"
pipeline_id: "slice.lightweight-tdd-development"
step_id: "slice-tdd-cycle-runner"
entry_gate: false
public_trigger: false
native_skill_discovery: false
body_path: "capabilities/instructions/pipelines/slice-variants/lightweight-tdd/slice-tdd-cycle-runner.step.md"
source_manifest: "capabilities/pipelines/slice-variants/lightweight-tdd.pipeline.json"
legacy_skill_ref: "slice-tdd-cycle-runner"
architecture_refs:
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#lightweight-development"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19"
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#lightweight-and-debug-slice-variants"
---

# Slice TDD Cycle Runner

## Overview

This is a reference_adapter_skill for `superpowers:test-driven-development` inside Tect's Lightweight TDD Slice variant. Its core rule is test-first implementation with proof: create RED, verify the failure, make the smallest GREEN patch, verify the pass, then refactor only while proof stays green.

## When to Use

Use this after `slice-test-target-selector` has identified the smallest meaningful failing test or acceptance proof target for a selected `slice.lightweight-tdd-development` Slice.

Use it when the change is small and understood, acceptance checks are clear, affected files are bounded, workspace preflight has not blocked edits, and focused local proof is sufficient.

Do not use it when root cause is unknown, no test target exists, the change requires design-spec or decisions artifacts, live/deploy proof dominates, repeated failures have appeared, or architecture ambiguity requires full design-to-execution, debug/root-cause, or hybrid implementation/ops escalation.

## Source Contract

- Record ID: `tect-skill.slice-lightweight-debug.slice-tdd-cycle-runner`
- Pipeline manifest: `capabilities/pipelines/slice-variants/lightweight-tdd.pipeline.json`
- Resolution state: `tect_skill`
- Owning manifest: `capabilities/pipelines/slice-variants/lightweight-tdd.pipeline.json`
- Manifest step: `step_graph.steps.slice-tdd-cycle-runner`
- Gate: `red_green_refactor_proof`
- Output: `tdd-notes.md`
- Terminal state for this step: `implemented_locally`
- Architecture anchors: `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6`, `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19`, `docs/architecture/master-plugin-target-architecture-part-5-pipeline-fabric.html`, `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#lightweight-and-debug-slice-variants`, `docs/architecture/master-plugin-target-architecture-validation-harness-evals.html`
- Atom anchors: `pipeline.slice.lightweight_tdd.tdd.cycle.runner`, `pipeline.slice.lightweight-tdd`
- External reference adapted: `skills/references/superpowers/test-driven-development/SKILL.md`

Lightweight means lower ceremony, not lower proof. This step can implement locally only inside the active Slice authority boundary; completion, deployment, live validation, result writing, and promotion belong to later steps.

Authority boundary: this skill defines the lightweight TDD step contract and does not grant authority by itself. Source edits require the active Slice authority state and workspace preflight to permit them. It does not authorize pipeline execution outside the selected runtime state, active state persistence, branch mutation, package installation, deployment, live-system commands, durable-domain writes, result writing, promotion, maintenance execution, or team merge behavior.

## Operating Procedure

Keep active behavior centered on the TDD proof loop, not on registry bookkeeping. Adapt `superpowers:test-driven-development` discipline into Tect's Slice lifecycle instead of copying or delegating to the external skill as canonical runtime behavior.

1. Confirm the preflight gate from the active Slice context before patching: selected Lightweight TDD variant, authority permitting bounded source edits, workspace preflight recorded in `workspace-preflight.md`, clear acceptance checks, and a named target in `test-target.md`. If any item is missing, stop before source mutation.
2. Choose the acceptance target for this cycle: one user-visible behavior, regression, or edge case already declared by the Slice. Prefer the smallest useful test that can fail for that behavior. Split any target containing "and" into separate cycles.
3. Read the selected target from `test-target.md`, then start `tdd-notes.md`: acceptance behavior, target test file or command, expected RED failure, affected source boundary, command boundary, and escalation triggers for ambiguity, missing tests, deploy/live risk, or repeated failure.
4. Create exactly one RED test or acceptance proof for the next behavior. It must exercise real code where practical, have a clear behavioral name, and fail because the behavior is absent rather than because of syntax, setup, or fixture mistakes.
5. Verify RED with the focused command named in the Slice. Allowed commands are local test/build/typecheck commands needed for the target. Do not install packages, mutate git state, call remote services, deploy, run live probes, or broaden into unrelated verification. If the test passes immediately, errors for the wrong reason, or cannot be run, fix the test/proof target or block; do not write implementation code.
6. Make the smallest GREEN patch inside the bounded affected source surface. The active authority state must already allow the edit. Do not add adjacent features, speculative abstractions, broad refactors, package installs, git operations, deployment commands, live probes, or unrelated cleanup.
7. Verify GREEN with the same focused command, then any declared affected local command if the Slice contract requires it. If it fails, adjust the implementation, not the RED expectation, unless the RED failure was proven invalid and is recorded.
8. Refactor only after GREEN. Keep behavior unchanged, keep the patch within the declared boundary, and rerun focused proof after each meaningful cleanup.
9. Reject test anti-patterns as blockers, not shortcuts: tests written after implementation, tests that only assert mock calls, test-only production hooks, over-mocking unknown dependencies, broad snapshot assertions, hidden manual-only proof, and changing the test expectation to match the patch.
10. Repeat RED/GREEN/REFACTOR only for acceptance checks already declared in the lightweight Slice. If new uncertainty, cross-component design, root-cause investigation, or operational proof appears, stop and route to the correct Slice variant.
11. Finish this step by updating `tdd-notes.md` with nonempty fields `selected_test_target:`, `red_command:`, `red_exit_code:`, `red_assertion:`, `source_change_ref:`, `green_command:`, `green_exit_code:`, `green_assertion:`, and `target_binding:`. When and only when those fields truthfully bind the selected target, observed RED, bounded source change, and observed GREEN, add exact attestations `selected_test_identity_recorded: true`, `red_command_evidence_recorded: true`, `red_failure_observed: true`, `source_change_identity_recorded: true`, `green_command_evidence_recorded: true`, `green_pass_observed: true`, and `same_target_binding_verified: true`, followed by exact lines `gate: red_green_refactor_proof` and `terminal_state: implemented_locally`. Empty, placeholder, planned, expected, or not-run fields forbid every success attestation. Include refactor proof if used, deviations, residual risk, and next-step handoff. Do not update `result.md`; do not claim completed_local_verified until the later verification runner records `verification.md`.

## Outputs

The primary output is `tdd-notes.md` for the selected Slice. It must contain the selected target, typed RED/GREEN/source/binding fields, and truth-only execution attestations listed above, plus the acceptance target, smallest useful test, command boundary, RED proof, GREEN proof, refactor proof when used, affected files, bounded patch statement, skipped/deferred checks, and escalation decision if the lightweight path stopped. Planning prose in `test-target.md` and empty field labels are never execution proof.

`workspace-preflight.md` is pre-edit evidence context for this step. Hand off local command summaries so `slice-lightweight-verification-runner` can later write the separate `verification.md` with focused and affected proof. This skill does not write final verification verdicts. The result boundary is strict: it may leave local implementation changes only when authority and workspace preflight allow them; it must not write `result.md`, `deployment-validation.md`, promotion records, heavyweight design artifacts, or hidden second lifecycle folders.

## Verification

Content verification requires a visible RED/GREEN/refactor trace: the RED test failed for the expected missing behavior, GREEN passed after the minimal patch, any refactor stayed green, and the command boundary was not exceeded. Trigger verification must show at least two positive lightweight-TDD scenarios and one route-away scenario. Harness verification is `node tools/validate-internal-skill-body-quality.mjs --skill slice-tdd-cycle-runner` plus `node tools/validate-internal-skill-trigger-fixtures.mjs --skill slice-tdd-cycle-runner`. Runtime proof for a real Slice is still local-only until later verification/result steps record higher truth.

Registry grounding verification is `node tools/validate-internal-skill-resolutions.mjs`; it must find the record ID, pipeline manifest, and resolution state in the Source Contract while body-quality still sees a real TDD operating procedure rather than wrapper-only prose.

## Failure Modes

Block with a missing-test-target state when no meaningful RED proof can be selected. Block with `blocked_missing_authority` when source edit authority or workspace preflight is absent. Escalate to full design-to-execution when acceptance, architecture, component boundaries, or required artifacts exceed lightweight scope. Escalate to debug/root-cause when observed behavior is unexplained or a GREEN attempt exposes unknown cause. Escalate to hybrid implementation/ops when deployment, live validation, production authority, rollback, or operational side effects dominate. Stop after repeated RED/GREEN failures instead of stacking guesses.

Terminal states are limited to `implemented_locally` for this step, or a blocked/escalated handoff state when the cycle cannot proceed. Never claim `completed_local_verified`, deployment success, live health, result readiness, or promotion from this step.

## Quick Reference

RED first. GREEN minimally. REFACTOR only after proof. Stop instead of expanding the Slice.
