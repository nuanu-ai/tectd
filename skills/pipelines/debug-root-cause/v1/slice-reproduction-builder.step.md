---
id: "slice-reproduction-builder"
kind: "internal_step"
owner_family: "slice_variant"
pipeline_id: "slice.debug-root-cause"
step_id: "slice-reproduction-builder"
entry_gate: false
public_trigger: false
native_skill_discovery: false
body_path: "capabilities/instructions/pipelines/slice-variants/debug-root-cause/slice-reproduction-builder.step.md"
source_manifest: "capabilities/pipelines/slice-variants/debug-root-cause.pipeline.json"
legacy_skill_ref: "slice-reproduction-builder"
architecture_refs:
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#debug-root-cause-variant"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19"
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#lightweight-and-debug-slice-variants"
---

# Slice Reproduction Builder

## Overview

This skill turns a captured debug symptom into the smallest reliable reproduction record for the Debug/root-cause Slice. It is the debug/root-cause reproduction step only: prove the observed/expected delta with bounded evidence, or record why it cannot currently be reproduced. No hypothesis, root cause, or fix may advance from this step without that reproduction truth.

## When to Use

Use this when:

- `slice.md` selects `slice.debug-root-cause`;
- `symptom.md` exists and names the symptom input, expected behavior, observed behavior, affected surface, environment, and any timestamps or source links available;
- the next gate is `reproduction_or_unable_to_reproduce_recorded`;
- a later worker needs a repeatable test, command, runtime path, API call, log query, trace, screenshot sequence, or read-only live probe before evidence planning.

Do not use this when the issue is already root-caused, when evidence ordering has already started from a valid reproduction, when live incident or ops authority dominates, when the next step is recent-change inspection, data-flow tracing, root-cause decision, fix planning, or verification, or when the user is asking to implement a fix.

## Source Contract

Grounding and boundaries:

- `capabilities/pipelines/slice-variants/debug-root-cause.pipeline.json` step `slice-reproduction-builder`
- `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6`
- `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19`
- `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#debug-root-cause-variant`
- `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#lightweight-and-debug-slice-variants`
- `docs/architecture/master-plugin-target-architecture-part-5-pipeline-fabric.html`
- `docs/architecture/master-plugin-target-architecture-validation-harness-evals.html`
- `skills/references/superpowers/systematic-debugging/SKILL.md`

The manifest step produces `reproduction.md`, gates on `reproduction_or_unable_to_reproduce_recorded`, ends in `reproduction_ready` or `unable_to_reproduce`, and on failure routes to `record_unable_to_reproduce_or_block`. This is a reference adapter skill for `superpowers:systematic-debugging`: adapt its reproduce-consistently, evidence, hypothesis, and root cause discipline, but do not copy its full body or treat it as the canonical Tect implementation.

This skill grants no source mutation, deployment, package install, or live-system command authorization. It may document authorized read-only commands or probes already allowed by the active Slice contract; otherwise it records proposed steps and the missing authority.

## Operating Procedure

1. Read `symptom.md` and `slice.md` before reproduction work. Extract the symptom input, expected result, observed result, environment, affected surface, source freshness, authority boundary, and any existing evidence links. If either artifact is missing or the observed/expected delta is not stated, route back to symptom capture or contract repair.
2. State the current truth before trying anything: what is known from source evidence, what is unverified, what is stale, and what cannot be claimed. Include the root-cause-not-established statement.
3. Choose the least invasive reproduction target that can show the delta. Prefer an existing failing test or local command when available; otherwise use a precise runtime path, API request, log query, trace, screenshot sequence, or authorized read-only live probe. Record exact command, test, runtime path, API call, log query, or read-only live probe, including working directory, inputs, flags, versions, environment variables that affect behavior, credentials boundary, and expected output.
4. Separate observation from hypothesis. Write what the reproduction would prove, what it would not prove, and which hypothesis questions remain open. Do not guess root cause, do not rank likely fixes, and do not attempt a fix.
5. Execute only if the active Slice contract already authorizes the action. For anything else, write the proposed step, required authority, risk, owner needed, and why execution is blocked. Never self-authorize source mutation, branch changes, package installation, deployment, destructive operations, or live-system writes from this skill.
6. Minimize the reproduction. Remove optional setup until only the shortest setup, action, assertion, and environment remain. Preserve any fixture, seed data, commit, service URL, timestamp range, account class, feature flag, config, or cleanup note required for repeatability.
7. Classify the outcome. If the symptom repeats and the delta is visible, set terminal state `reproduction_ready`. If bounded authorized attempts fail, set terminal state `unable_to_reproduce` and include attempts, outputs, missing inputs, environmental differences, freshness limits, and the next evidence options. If required source truth or authority is absent, use `record_unable_to_reproduce_or_block` and hand off instead of broadening into guesses.
8. Prepare the evidence-log handoff without doing the next step's work. Add a compact block for `evidence-log.md`: reproduction target, result, artifact/output paths, timestamps, confidence, open questions, and recommended next route. The next route is normally `slice-evidence-order-planner`; if the reproduction exposes recent-change dependence, route to `slice-recent-change-inspector`; if authority or environment blocks progress, route to `slice-debug-handoff-builder` or ops/incident handling.

## Outputs

Produce `reproduction.md` or the active debug Slice reproduction section. It must contain:

- source references: `slice.md`, `symptom.md`, owning manifest step, and source evidence used;
- symptom input, expected behavior, observed behavior, and the observed/expected delta;
- environment and preconditions: workspace, branch/commit if known, service/runtime version, config, data fixture, account/token class, timestamp window, and authority boundary;
- minimal setup, action, assertion, and exact reproduction command or steps, or the exact proposed step if execution is not authorized;
- outputs and artifacts: terminal output summary, log query result, screenshot/trace path, command exit code, request/response shape, or reason no output exists;
- current truth: what is reproduced, what is not reproduced, what remains unknown, and why root cause is not yet established;
- unable-to-reproduce record when applicable: bounded attempts, deltas from the reported environment, missing input, stale source risk, next evidence target, and blocker owner;
- evidence-log handoff block for `evidence-log.md` with the reproduction result, confidence, open hypothesis questions, and next route;
- terminal state exactly `reproduction_ready`, `unable_to_reproduce`, or blocked through `record_unable_to_reproduce_or_block`.

## Verification

Verify the record against the manifest gate before advancing:

- A later worker can repeat the reproduction exactly or understand why it is currently impossible.
- The observed/expected delta, environment, command or steps, source freshness, and current truth are explicit.
- Evidence is cited; hypothesis language is marked as unproven; no root cause is declared; no fix, mutation, deploy, package install, or live write is proposed.
- The record names `slice.debug-root-cause`, `capabilities/pipelines/slice-variants/debug-root-cause.pipeline.json`, the `slice-reproduction-builder` step, the gate `reproduction_or_unable_to_reproduce_recorded`, and one allowed terminal state.
- The evidence-log handoff is enough for `slice-evidence-order-planner` to order the next inspection without re-asking what was tried.

For fixture validation, this body must keep the required Layer 6B sections, include owning architecture and manifest paths, adapt `superpowers:systematic-debugging` markers for evidence, hypothesis, and root cause discipline, and avoid wrapper-only boilerplate.

## Failure Modes

Block or route when:

- `symptom.md` lacks a concrete observed/expected delta: route to `slice-symptom-capture`.
- `slice.md` does not authorize the target action or does not select `slice.debug-root-cause`: route to contract repair, ops, or the selected variant.
- Reproduction requires source mutation, package install, deployment, credentials, destructive operations, or live-system writes: block or hand off with required authority.
- The issue is a live incident or operational recovery problem: route to operational execution or incident handling rather than normal debug reproduction.
- The reproduction would require speculative broad exploration: stop and record the narrow missing input instead.
- Authorized attempts do not reproduce the symptom: write `unable_to_reproduce`, preserve the current truth, and route to evidence planning or handoff.

Never convert inability to reproduce into a root cause claim. Never stack guesses, jump to architecture redesign, write a regression test, or proceed to fix work from this skill; those belong to later debug steps after the root-cause decision gate.
