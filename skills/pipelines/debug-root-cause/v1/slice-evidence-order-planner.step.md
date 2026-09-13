---
id: "slice-evidence-order-planner"
kind: "internal_step"
owner_family: "slice_variant"
pipeline_id: "slice.debug-root-cause"
step_id: "slice-evidence-order-planner"
entry_gate: false
public_trigger: false
native_skill_discovery: false
body_path: "capabilities/instructions/pipelines/slice-variants/debug-root-cause/slice-evidence-order-planner.step.md"
source_manifest: "capabilities/pipelines/slice-variants/debug-root-cause.pipeline.json"
legacy_skill_ref: "slice-evidence-order-planner"
architecture_refs:
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#debug-root-cause-variant"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19"
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#lightweight-and-debug-slice-variants"
---

# Slice Evidence Order Planner

## Overview

This is a reference adapter skill for `superpowers:systematic-debugging`, narrowed to Tect's debug/root-cause Slice. Its core rule is: convert symptom, reproduction, and debug context into the Evidence Order section of `evidence-log.md` before anyone stacks guesses, mutates source, deploys, or claims a root cause.

Evidence order is not a hypothesis ledger and not a fix plan. It is the proof sequence that says what to inspect, why it comes now, what it would prove or disprove, how fresh the source is, and what terminal handoff follows each result.

## When to Use

Use this when a debug/root-cause Slice has captured observed behavior, expected behavior, and a reproduction path or explicit reproduction boundary, but the next checks are unordered, risky, stale, or drifting toward fixes. It also applies when current logs, code, diffs, memory, and live state are mixed together and need a least-invasive-first sequence before hypothesis work.

Do not use this for initial symptom capture, building the first reproduction, maintaining `hypotheses.md`, declaring root cause, choosing a fix, writing a regression test, or verifying an implemented fix. Route active live incidents, deployment authority questions, rollback work, or operator actions to the operational or hybrid Slice path instead.

## Source Contract

Grounding:

- `capabilities/pipelines/slice-variants/debug-root-cause.pipeline.json#step_graph.steps.slice-evidence-order-planner`
- `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#debug-root-cause-variant`
- `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#lightweight-and-debug-slice-variants`
- `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6`
- `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19`
- `docs/architecture/master-plugin-target-architecture-capability-atom-shard-map.html#pipeline.slice.debug_root_cause.evidence.order.planner`
- `docs/architecture/master-plugin-target-architecture-validation-harness-evals.html`

Manifest contract: produce `evidence-log.md`, satisfy gate `evidence_order_declared`, block with `block_stacked_guesses`, and finish at `evidence_plan_ready`. The executable evidence-order plan is a required section inside `evidence-log.md`, not a separate divergent artifact.

Reference source: `skills/references/superpowers/systematic-debugging/SKILL.md`. Adapted behaviors are root cause before fix, read errors fully, reproduce first, inspect recent changes, gather evidence across component boundaries, trace data flow, test one hypothesis at a time, and stop after repeated failed fixes instead of adding another guess.

## Operating Procedure

1. Gate the inputs. Require active debug/root-cause Slice identity, symptom reference, observed behavior, expected behavior, reproduction command/path or explicit unable-to-reproduce boundary, parent Scope constraints, and authority for every source class to inspect. If the symptom is vague, route to symptom capture. If reproduction is missing or contradicted, return `reproduction_needed` and route to `slice-reproduction-builder`.
2. Define evidence classes from errors, reproduction output, tests, code, diffs, commits, configuration, dependencies, logs, runtime state, working examples, memory, and live probes. Include exact error text and stack traces, failing command output, user-visible behavior, relevant code/data-flow boundaries, git diff, recent commits, config/env/dependency changes, existing diagnostics, similar working paths, historical notes, and live or credentialed checks if they may be needed.
3. Convert classes into neutral evidence questions. Each planned check must say what it would prove, what it would disprove, which candidate root-cause area it informs, and what result would change the next step. The plan keeps evidence collection hypothesis-neutral: do not order checks to justify a favored theory, and do not let a likely cause become a root-cause claim.
4. Label current truth and freshness. For every source, mark current command output, current repository state, current runtime/live probe, recent log, historical log, memory/session note, stale report, or unverified claim. Each row labels source freshness and current-truth distance. State current-truth versus stale memory explicitly. Memory, old session notes, and stale reports are routing hints until refreshed against current evidence.
5. Use least-invasive-first ordering. Order checks by cost and risk, reversibility, freshness, blast radius, and authority. Start with cheap read-only local checks: exact error/reproduction output, current diff, recent commits, relevant code paths, config/dependency facts, existing logs, and similar working examples. Order cheapest and reversible checks before risky or live checks. Move to focused reruns, deeper data-flow tracing, expensive diagnostics, network calls, credentialed checks, and live probes only when earlier evidence cannot discriminate. This keeps risky or live checks gated behind authority.
6. Record proof/disproof and next routes. For each check, write the expected result that strengthens a candidate, the expected result that weakens or rejects it, and the next route: more evidence, `slice-hypothesis-ledger`, `slice-data-flow-tracer`, `slice-working-example-comparator`, `slice-root-cause-decision`, or handoff.
7. Track root-cause hypotheses without executing fixes. This skill may list candidate areas and hypothesis links, but it must not choose a fix, write implementation steps, run deployments, mutate source, or let several guesses proceed in parallel. If prior fixes failed three or more times, stop normal evidence ordering and route to architecture discussion or full design-to-execution assessment.
8. Handle contradictions by pausing and replanning. The planner handles contradictions by pausing and replanning before any new action. If evidence conflicts with the symptom, reproduction, logs, memory, or a recent-change story, stop the sequence, record the contradiction, discard stale-only support, add the smallest clarifying check, and choose a terminal handoff before continuing.
9. Declare terminal state. Use `evidence_plan_ready` only when the Evidence Order section of `evidence-log.md` contains every required check with source, class, freshness/current-truth label, proof/disproof expectation, cost/risk label, authority gate, hypothesis link, and next route. Use `block_stacked_guesses` when a fix or root-cause claim is being attempted without that proof. Use `handoff_required` when the next useful check needs access, authority, live risk, or mutation outside this step.

## Outputs

Write or update `evidence-log.md` for the active debug/root-cause Slice. Its Evidence Order section must contain:

- Context basis: symptom, expected behavior, observed behavior, reproduction reference, Scope constraints, authority boundary, and prior failed fixes.
- Evidence class inventory: each source with freshness/current-truth label, owner, access status, and whether it is current truth, stale memory, historical evidence, or unverified claim.
- Ordered plan: sequence number, evidence class, source/check, why this check comes now, proof expectation, disproof expectation, cost/risk/reversibility label, authority gate, affected hypothesis area, and next route.
- Contradictions and gaps: conflicting evidence, stale-only claims, missing access, unsafe checks, and the smallest clarifying check.
- Terminal state and handoff: one of `evidence_plan_ready`, `reproduction_needed`, `block_stacked_guesses`, or `handoff_required`.

Successful use produces or updates `evidence-log.md` with terminal `evidence_plan_ready`, paired with gate `evidence_order_declared`. It hands off to reproduction, hypothesis, data-flow tracing, or root-cause decision steps as dictated by the next route. This skill may describe read-only evidence checks, but it does not execute fixes, mutate source, deploy, run live-system commands, or authorize implementation.

## Verification

Verify trigger fit by checking that the Slice is already in debug/root-cause mode, has symptom and reproduction inputs, and needs evidence ordering before hypothesis ledger, data-flow tracing, root-cause decision, or fix strategy. Verify content by confirming `evidence-log.md` uses least-invasive-first ordering in its Evidence Order section, records current-truth versus stale memory, keeps evidence collection hypothesis-neutral, defines proof/disproof for every check, orders checks by cost and risk, gates risky/live checks behind authority, forbids fixes and guesses before discriminating evidence exists, and records terminal states `evidence_plan_ready`, `reproduction_needed`, `block_stacked_guesses`, and `handoff_required`.

Static validation should pass:

- `node tools/validate-internal-skill-body-quality.mjs --skill slice-evidence-order-planner`
- `node tools/validate-internal-skill-trigger-fixtures.mjs --skill slice-evidence-order-planner`

Manual review should reject any artifact that treats stale memory as current truth, puts live or destructive checks before local read-only checks, omits proof/disproof, names a root cause too early, or lets a fix begin before evidence can discriminate alternatives.

## Failure Modes

Return `reproduction_needed` when symptom and reproduction inputs are absent, vague, stale, or contradicted. Route back to symptom capture or reproduction builder instead of inventing evidence order from guesses.

Return `block_stacked_guesses` when the user or agent asks for a fix, root-cause declaration, implementation plan, source mutation, deployment, or live-system action before discriminating evidence exists. Name the missing evidence, freshness problem, authority gap, or contradiction directly.

Return `handoff_required` when the next useful check needs credentials, live-system authority, risky production probing, destructive action, package installation, operator decision, or broader architecture discussion. If three or more prior fixes failed, stop normal debug ordering and route to architecture discussion or full design-to-execution assessment.

## Quick Reference

Symptom plus reproduction. Evidence classes. Current truth beats stale memory. Least-invasive first. Proof and disproof for every check. No proof, no fix.
