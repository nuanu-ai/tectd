---
id: "slice-lightweight-entry-gate"
kind: "internal_step"
owner_family: "slice_variant"
pipeline_id: "slice.lightweight-tdd-development"
step_id: "slice-lightweight-entry-gate"
entry_gate: true
public_trigger: false
native_skill_discovery: false
body_path: "capabilities/instructions/pipelines/slice-variants/lightweight-tdd/slice-lightweight-entry-gate.step.md"
source_manifest: "capabilities/pipelines/slice-variants/lightweight-tdd.pipeline.json"
legacy_skill_ref: "slice-lightweight-entry-gate"
architecture_refs:
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#lightweight-development"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19"
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#lightweight-and-debug-slice-variants"
---

# Slice Lightweight Entry Gate

## Overview
This skill decides whether a Slice candidate is allowed to enter the Lightweight TDD lifecycle. Its core rule is fit before speed: lightweight means smaller artifact ceremony, never weaker proof, hidden ambiguity, or skipped escalation.

## When to Use
Use this after Kernel and Runtime have selected `slice.lightweight-tdd-development` as the candidate path and before lightweight intent capture begins. The request should look like a small understood code, config, or business-rule change with a parent Scope or Slice candidate, visible acceptance checks, bounded surface, and focused local proof.

Use it when the next decision is whether the work may continue through `selected_lightweight_variant` to `ready_for_intent_capture`, or whether it must route out before downstream lightweight steps create the wrong lifecycle.

Do not use this for unclear acceptance behavior, unknown-cause bugs, architecture ambiguity, broad component design, deploy or live validation as completion proof, missing test targets, repeated failures, operations, research, procedure capture, maintenance/drift repair, result writing, promotion, or current-state lookup. Route those to full, debug, hybrid, ops, research, procedure, maintenance, result, promotion, or query paths.

## Source Contract
Ground this behavior in `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#lightweight-development`, `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#lightweight-and-debug-slice-variants`, `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6`, and `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19`.

The owning manifest is `capabilities/pipelines/slice-variants/lightweight-tdd.pipeline.json`. The required step is `slice-lightweight-entry-gate`, invoked by `pipeline.slice.lightweight-tdd`, producing the entry-gate payload for `slice.md`, gated by `selected_lightweight_variant`, ending in `ready_for_intent_capture`, and failing through `escalate_full_or_block`. Its entry contract requires user request, parent Scope or Slice candidate, acceptance checks, authority state, workspace preflight summary, kernel assessment, runtime variant-selection record, slice parent integration, and declared authority. No external skill body is a source for this skill.

## Operating Procedure
1. Read the candidate runtime packet at entry depth only: user request, parent Scope or Slice candidate, acceptance checks if present, authority state, workspace preflight summary or explicit gap, kernel assessment, runtime variant-selection record, and any slice parent integration note.
2. Confirm the source inputs exist. `user request`, `parent Scope or Slice candidate`, `acceptance checks`, `authority state`, and `workspace preflight summary` must be present or explicitly marked as gaps. Required gates are kernel assessment, runtime variant selection, slice parent integration, and authority declared.
3. Pass lightweight fit only when every entry check is true: the change is small and understood, acceptance behavior is clear enough for a focused test or proof target, affected files or components are bounded, local proof can validate the claim, and no live or deployment proof is required for completion.
4. Test hard rejection signals from the manifest and final map: architecture ambiguity, unknown root cause, cross-component uncertainty, missing test target, deploy or live validation requirement, repeated failure, unsafe authority state, missing parent boundary, research need, operation execution, procedure capture, promotion request, maintenance/drift request, or current-state query.
5. Check containment. The candidate must belong under an existing or explicitly proposed parent Scope or Slice boundary. If the parent is absent, contradictory, too broad, or only implied by chat history, stop and request parent selection or route to full development rather than creating a hidden lightweight lifecycle.
6. Decide one terminal path. If the fit passes, return an entry-gate payload for the caller or contract-writer to place in `slice.md`: variant `slice.lightweight-tdd-development`, lifecycle depth `lightweight`, gate `selected_lightweight_variant`, accepted fit evidence, source-input gaps if any, authority/preflight status, proof expectation, next step `slice-lightweight-intent-capture`, and terminal state `ready_for_intent_capture`.
7. If any rejection signal is present, return `escalate_full_or_block` with the specific target route or blocker: `slice.full-design-to-execution` for ambiguity, broad design, missing test target, or cross-component uncertainty; `slice.debug-root-cause` for unknown cause or repeated failure; `slice.hybrid-implementation-operation` for code plus deploy/live proof; operational preparation or execution for ops authority or command work; research-to-durable-knowledge for evidence/research work; procedure capture for valuable repeated ad hoc procedure; maintenance for drift/stale/missing consistency work; query/freshness route for current-state lookup; blocked for missing authority, source truth, parent boundary, or unsafe state.
8. Stop at entry. Do not capture detailed intent, load broad context, select tests, run the TDD cycle, patch files, mutate source, verify completion, write results, deploy, promote, perform maintenance repair, execute operations, or run workspace, branch, package, team, or live-system actions from this step.

## Outputs
Return a `slice.md` entry-gate block or equivalent handoff payload; do not persist it from this skill. A passing output records the Lightweight TDD variant, parent Scope/Slice pointer, accepted fit signals, source-input completeness or gaps, authority and workspace-preflight posture, proof expectation, next step, and terminal state `ready_for_intent_capture`.

A failing output records `escalate_full_or_block`, the rejection signal, recommended target variant or blocker, and any missing source the caller must provide. It may route to full, debug, hybrid, ops preparation/execution, research, procedure capture, maintenance, query/freshness, or blocked state. It preserves unclear, stale, deferred, or contradictory claims instead of smoothing them into lightweight acceptance.

## Verification
Verify trigger compliance by checking that the scenario is a small clear Slice candidate at variant-entry time, not downstream intent capture, context loading, TDD execution, patching, verification running, result writing, deployment, promotion, query, research, maintenance, procedure capture, or operations work. Verify content by confirming the output names the manifest step, entry inputs, gate, terminal state, parent boundary, bounded-change checks, proof basis, authority posture, next step, and escalation reason.

Run the Layer 6B checks for this skill: `node tools/validate-internal-skill-body-quality.mjs --skill slice-lightweight-entry-gate` and `node tools/validate-internal-skill-trigger-fixtures.mjs --skill slice-lightweight-entry-gate`. The body must retain the owning manifest path, at least one architecture HTML path, the exact required H2 sections, and no wrapper-only headings or forbidden execution authorization.

## Failure Modes
Block or route out when the request is broad, cross-component, architecture-sensitive, missing a parent boundary, missing acceptance checks, missing a local proof target, dependent on deployment/live validation, or reporting behavior with unknown root cause. Accepting these would create false proof and a hidden second lifecycle.

Block when authority, workspace state, source truth, or current context cannot be established enough to decide variant fit. If only the workspace preflight is missing but all other signals are safe, state the preflight gap and hand to the proper preflight or parent-routing step rather than pretending the gate passed.

Do not downgrade proof because the change feels small. If a user insists on lightweight while rejection signals remain, record the waived preference separately and route to the safer variant or blocker; user preference can choose a narrower path only when it does not fake proof, freshness, authority, or consequence.

Keep the failure response as a zero-action boundary: name the rejected signal, name the safer route, and stop before downstream lightweight artifacts make the wrong path look accepted. This skill never runs TDD, patches, verifies, writes a result, deploys, promotes, repairs maintenance drift, answers the query, or executes the operation after routing.
