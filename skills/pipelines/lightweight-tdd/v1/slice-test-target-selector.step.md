---
id: "slice-test-target-selector"
kind: "internal_step"
owner_family: "slice_variant"
pipeline_id: "slice.lightweight-tdd-development"
step_id: "slice-test-target-selector"
entry_gate: false
public_trigger: false
native_skill_discovery: false
body_path: "capabilities/instructions/pipelines/slice-variants/lightweight-tdd/slice-test-target-selector.step.md"
source_manifest: "capabilities/pipelines/slice-variants/lightweight-tdd.pipeline.json"
legacy_skill_ref: "slice-test-target-selector"
architecture_refs:
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#lightweight-development"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19"
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#lightweight-and-debug-slice-variants"
---

# Slice Test Target Selector

## Overview

Select the smallest meaningful RED target for a Lightweight TDD Slice. The core rule is: no local implementation step may start until the Slice has a named test or acceptance proof target, an expected failure, and a gap record when no good target exists.

## When to Use

Use this after `slice-lightweight-contract-writer` has declared a selected `slice.lightweight-tdd-development` Slice, acceptance behavior is clear, context and workspace preflight are available, and the next decision is which focused test or proof should fail before implementation.

Use it for small code, config, or business-rule changes where an existing test, new colocated test, command-level assertion, fixture, contract check, or manual acceptance proof can constrain the next patch.

Do not use it when the active work is an unexplained bug/regression, architecture design question, cross-repo or cross-component change, deploy/live validation task, final verification, result writing, promotion, or the actual RED/GREEN/refactor cycle. Route those to debug/root-cause, full design-to-execution, hybrid implementation plus operation, lightweight verification/result, or promotion steps.

## Source Contract

Grounding sources:

- `capabilities/pipelines/slice-variants/lightweight-tdd.pipeline.json` step `slice-test-target-selector`
- `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#lightweight-development`
- `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#lightweight-and-debug-slice-variants`
- `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6`
- `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19`
- `docs/architecture/master-plugin-target-architecture-capability-atom-shard-map.html#pipeline.slice.lightweight_tdd.test.target.selector`

The manifest step is required, invokes `slice-test-target-selector`, produces `test-target.md`, gates on `test_target_selected_or_gap_recorded`, reaches `test_target_ready`, and fails through `write_test_plan_or_escalate`. The following cycle alone owns `tdd-notes.md`. The relevant atom is `pipeline.slice.lightweight-tdd`; lightweight means lower ceremony, not lower proof.

Required source inputs for actual use:

- selected Slice folder and `slice.md` with variant `slice.lightweight-tdd-development`, intent, acceptance checks, authority, and escalation triggers
- context-loader notes or README with immediate files/docs/tests, parent Scope constraints, relevant deferred/proof notes, and source boundaries
- workspace preflight summary showing whether bounded artifact writes are allowed and whether source edits need isolation
- nearby proof surfaces inspected read-only: existing tests, fixtures, package scripts, validators, snapshots/goldens, CLI/API checks, docs acceptance criteria, or credible manual proof path
- pipeline manifest step contract above, especially gate `test_target_selected_or_gap_recorded`, terminal state `test_target_ready`, and failure route `write_test_plan_or_escalate`

## Operating Procedure

1. Confirm entry readiness: the active Slice is Lightweight TDD, `slice.md` names the intent and acceptance checks, context is current enough, workspace preflight does not block the Slice artifact update, and escalation triggers have not fired. Stop if the request now needs debug, full design, hybrid ops, deploy/live proof, or broader authority.
2. Extract the smallest behavior that can fail first. Tie it to one acceptance check, user-visible behavior, contract invariant, bug-prevention assertion, or config/business rule. If the behavior spans multiple unrelated assertions, split it and select the first meaningful one.
3. Inspect nearby proof surfaces read-only: existing tests, colocated fixtures, package scripts, validator commands, snapshot/golden files, API or CLI checks, docs acceptance criteria, and prior proof/deferred notes. Prefer a real automated test over a manual proof, and prefer the narrowest existing command over a broad suite.
4. Choose the target shape. Record the test file or command, behavior name, setup or fixture needed, expected RED failure message or missing assertion, affected source boundary, and why this target is sufficient for the next TDD cycle. If the target is a new test, name the intended file and assertion without writing it.
5. If no useful automated target exists, record the gap instead of pretending. Define the best acceptance proof, explain why no test target is currently available, list the smallest `test-plan.md` task needed, and route through `write_test_plan_or_escalate` when the gap makes lightweight unsafe.
6. Write only `test-target.md` when the selected Slice already grants artifact-write authority. Record the exact lines `gate: test_target_selected_or_gap_recorded` and `terminal_state: test_target_ready`, selected target, RED expectation, focused command or proof to collect, target scope, non-targeted checks, missing test-plan work, escalation decision, and handoff to `slice-tdd-cycle-runner`. If artifact writes are not allowed, emit the same content as a handoff payload for Runtime to persist.
7. End with `test_target_ready` only when a future agent can create or run the RED target without inventing behavior. Do not write `tdd-notes.md`, edit production/source code, write tests, create implementation patches, run RED/GREEN/refactor, run broad verification, write `implementation-notes.md`, write `verification.md`, write `result.md`, deploy, validate live behavior, promote knowledge, or claim the Slice is implemented from target selection alone.

## Outputs

Primary output is `test-target.md` for the selected Slice. It must name the selected test or acceptance proof target, behavior under test, RED expectation, focused command or proof method, affected file boundary, required fixture/setup, reason this is the smallest meaningful target, and any non-targeted checks deferred to later verification. It must contain exact lines `gate: test_target_selected_or_gap_recorded` and `terminal_state: test_target_ready` only when ready.

Use this shape:

- `selected_target`: existing test, new colocated test to write next, command assertion, fixture/contract check, or temporary acceptance proof
- `behavior_under_test`: one acceptance behavior or invariant
- `red_expectation`: expected failing assertion, missing assertion, error text, or proof observation before implementation
- `focused_command_or_proof`: narrow command to run, or exact manual proof when automation is unavailable
- `affected_boundary`: source files/modules/config touched by the future implementation, not edits performed now
- `setup_or_fixture`: data, fixture, environment, or precondition required
- `why_minimal`: why this target is sufficient for the next TDD cycle
- `deferred_checks`: broader verification intentionally left to later lightweight verification
- `gap_or_escalation`: missing target reason, `test-plan.md` task, handoff, or escalation target

When no meaningful target exists, the output is still a visible gap record in `test-target.md`: missing target reason, proposed `test-plan.md` or escalation route, authority or context gap, and whether the lightweight path may continue. This skill does not create `tdd-notes.md`, implementation changes, final verification, result, promotion, deployment, or live proof artifacts.

## Verification

Validate the skill body with `node tools/validate-internal-skill-body-quality.mjs --skill slice-test-target-selector` and trigger scenarios with `node tools/validate-internal-skill-trigger-fixtures.mjs --skill slice-test-target-selector`.

For real Slice use, verify that `test-target.md` satisfies the gate by answering: what exact behavior should fail, where the RED target lives, which command or proof exposes it, why the target is minimal, what setup is required, what source boundary the future change may touch, what checks are deferred, and what happens if the target is unavailable. Confirm the note preserves the manifest path, architecture grounding, exact terminal line `terminal_state: test_target_ready`, and the rule that local implementation and `tdd-notes.md` belong to `slice-tdd-cycle-runner`.

## Failure Modes

Block or escalate when acceptance behavior is unclear, target files or tests are unavailable, no automated or credible acceptance proof exists, workspace preflight blocks the Slice artifact update, authority is missing, live/deploy validation dominates, or the target would require broad design decisions before a RED proof can be named.

Route unknown cause to debug/root-cause, cross-component uncertainty to full design-to-execution, deploy/live dependency to hybrid implementation plus operation, missing test target to `write_test_plan_or_escalate`, and repeated target-selection or RED setup failure to escalation rather than stacked guesses. Terminal states are `test_target_ready` when a target or gap is recorded, `blocked_missing_target` when no credible proof exists, `blocked_missing_authority` when artifact recording is not allowed, and `handoff_required` when a different owner must continue.

Do not invent a passing target, broaden to a whole suite as a substitute for a focused RED, change production code while selecting the target, write the test inside this step, run implementation, run deployment or live-system commands, or treat a target-selection note as implementation or completion proof. If the only available proof is manual, label it as a temporary acceptance proof and require later automated coverage or an explicit residual-risk note. This skill has zero implementation authority; it names proof work so the next step can start safely.
