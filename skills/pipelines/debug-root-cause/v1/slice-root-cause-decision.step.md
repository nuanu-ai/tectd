---
id: "slice-root-cause-decision"
kind: "internal_step"
owner_family: "slice_variant"
pipeline_id: "slice.debug-root-cause"
step_id: "slice-root-cause-decision"
entry_gate: false
public_trigger: false
native_skill_discovery: false
body_path: "capabilities/instructions/pipelines/slice-variants/debug-root-cause/slice-root-cause-decision.step.md"
source_manifest: "capabilities/pipelines/slice-variants/debug-root-cause.pipeline.json"
legacy_skill_ref: "slice-root-cause-decision"
architecture_refs:
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#lightweight-and-debug-slice-variants"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19"
---

# Root-Cause Decision

## Overview

This is the decision gate for the Debug/root-cause Slice variant after symptom, reproduction, evidence order, and hypothesis work exist. It is a `reference_adapter_skill` for `superpowers:systematic-debugging`: no fix strategy, patch, or completion claim is allowed until the root cause mechanism is proven or the missing evidence is explicitly recorded.

## When to Use

Use this when a `slice.debug-root-cause` Slice has a candidate cause and the next question is whether the diagnostic packet is strong enough to advance past diagnosis. Required inputs are `symptom.md`, `reproduction.md` or an unable-to-reproduce record, `evidence-log.md`, and `hypotheses.md`; optional inputs include recent-change notes, traces, diagnostics, logs, or working-example comparisons.

Do not use this for initial symptom capture, reproduction building, evidence gathering, hypothesis discovery, fix strategy, regression-test writing, patch execution, deployment, or live incident recovery. Route those to the surrounding debug steps, operational variants, lightweight/full development, or handoff as appropriate.

## Source Contract

- Architecture anchors: `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6`, `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19`, `docs/architecture/master-plugin-target-architecture-part-5-pipeline-fabric.html`, `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#lightweight-and-debug-slice-variants`, and `docs/architecture/master-plugin-target-architecture-validation-harness-evals.html`.
- Atom anchor: `docs/architecture/master-plugin-target-architecture-capability-atom-shard-map.html#pipeline.slice.debug_root_cause.root.cause.decision`, which defines this step as declaring the root cause, confidence, source evidence, and why symptom-level fixes are insufficient.
- Manifest anchor: `capabilities/pipelines/slice-variants/debug-root-cause.pipeline.json` step `slice-root-cause-decision`; produces `root-cause.md`; gate `root_cause_proven_or_unknown_recorded`; failure action `block_fix_before_root_cause`; terminal states `root_cause_found` and `blocked_missing_evidence`.
- Classification and reference coverage: `reference_adapter_skill` for `superpowers:systematic-debugging`, adapted from `skills/references/superpowers/systematic-debugging/SKILL.md`. Preserve the source skill's root-cause-before-fix rule, hypothesis testing, evidence comparison, data-flow tracing, and stop after repeated failed fixes; do not copy the external body wholesale.

## Operating Procedure

1. Check the diagnostic packet before judging the cause. Require a concrete symptom, expected-vs-observed delta, `reproduction.md` or an unable-to-reproduce record, ordered `evidence-log.md`, and `hypotheses.md` with active, rejected, and still-open candidates. If any required input is absent or stale, stop in `blocked_missing_evidence`.
2. Compare alternatives instead of selecting the most convenient explanation. For each active hypothesis, name the claimed originating condition: code path, state transition, configuration, dependency, data flow, environment, recent change, or external constraint. Record direct support, contradicting evidence, missing checks, and assumptions.
3. Prove or reject the cause mechanism. A valid root cause must explain how the originating condition produces the observed bad value, control path, state, timing, output, or user-visible symptom. Build an evidence path from reproduction or unable-to-reproduce boundary through evidence-log entries, traces, logs, commits, working-example differences, or source reads to the mechanism.
4. Reject guesses and symptom-level explanations. Reject any candidate that only renames the visible failure, depends on untested intuition, ignores a contradictory working example, lacks a reproduction relationship, bundles several possible causes, or would justify fixing before the source mechanism is known.
5. Decide confidence and residual uncertainty. Mark `root_cause_found` only when one cause explains the observed behavior, survives comparison with alternatives, and has enough proof to hand off a single next strategy. If residual uncertainty changes the affected surface, fix lane, verification target, or authority need, mark `blocked_missing_evidence` instead.
6. Write `root-cause.md` with a complete decision record: terminal state, cause summary, cause mechanism, evidence path, reproduction relationship, confidence, residual uncertainty, rejected hypotheses with reasons, remaining proof gaps, why symptom-level fixes are insufficient, and allowed next step.
7. Enforce the boundary. If proven, hand off only to `slice-debug-fix-strategy`; this skill does not create `fix-plan.md` or run fixes. If unknown, keep `block_fix_before_root_cause` active and route to the exact missing evidence step or `slice-debug-handoff-builder`. If three or more fixes have already failed, route to architecture discussion or full development instead of approving another attempt.

## Outputs

Primary output is `root-cause.md`. It must state one of two outcomes: `root_cause_found` with the proven cause mechanism and evidence path, or `blocked_missing_evidence` with the exact evidence needed before diagnosis can continue.

For `root_cause_found`, include the cause summary, mechanism, source evidence path, reproduction relationship, rejected hypotheses, confidence, residual uncertainty, and handoff to `slice-debug-fix-strategy`. For `blocked_missing_evidence`, include the strongest candidate, why it is not proven, missing evidence, next diagnostic owner, and continued `block_fix_before_root_cause` state.

This skill does not create `fix-plan.md`, `patch.md`, `verification.md`, `result.md`, or `fix-before-root-cause.md`, and it does not authorize source mutation, deployment, live-system commands, or stacked fix attempts.

## Verification

Before selecting this skill, the trigger fixture must show at least two positive root-cause decision scenarios and one negative scenario that routes away before diagnosis is ready. Content validation must pass with:

```bash
node tools/validate-internal-skill-body-quality.mjs --skill slice-root-cause-decision
node tools/validate-internal-skill-trigger-fixtures.mjs --skill slice-root-cause-decision
```

For runtime use, verify `root-cause.md` answers: What is the cause mechanism? What evidence path proves it from reproduction to source? Which alternatives were rejected and why? What confidence and residual uncertainty remain? Why would a symptom patch be unsafe? Which terminal state and next step are now allowed?

## Failure Modes

Stop in `blocked_missing_evidence` when reproduction is absent without an unable-to-reproduce record, evidence is stale or unordered, hypotheses are not compared, source evidence contradicts the candidate, the evidence path does not reach the cause mechanism, or the decision would rely on intuition. Keep the fix boundary closed when a proposed answer only describes the symptom, names a broad subsystem without proof, bundles multiple possible causes into one conclusion, or leaves residual uncertainty that changes the fix strategy.

Route to earlier debug steps when the packet needs more evidence, to operational execution when recovery authority dominates, to full development when the cause requires redesign, and to architecture discussion when repeated fix attempts expose a broader pattern failure. If user pressure asks for a patch before proof, record the blocked state and the missing evidence rather than approving fix strategy.

## Quick Reference

Decision allowed: proven cause plus evidence. Decision blocked: unknown cause, weak hypothesis, stale proof, missing reproduction boundary, or pressure to patch first.
