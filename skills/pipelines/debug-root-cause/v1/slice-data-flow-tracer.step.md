---
id: "slice-data-flow-tracer"
kind: "internal_step"
owner_family: "slice_variant"
pipeline_id: "slice.debug-root-cause"
step_id: "slice-data-flow-tracer"
entry_gate: false
public_trigger: false
native_skill_discovery: false
body_path: "capabilities/instructions/pipelines/slice-variants/debug-root-cause/slice-data-flow-tracer.step.md"
source_manifest: "capabilities/pipelines/slice-variants/debug-root-cause.pipeline.json"
legacy_skill_ref: "slice-data-flow-tracer"
architecture_refs:
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#lightweight-and-debug-slice-variants"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19"
---

# Slice Data Flow Tracer

## Overview

This skill builds the trace record for a Debug/root-cause Slice when the symptom is visible but the originating boundary is still unknown. Core rule: map the bad value, control branch, or state transition backward through evidence until the first known source or a precise trace gap is recorded.

## When to Use

Use this after the Slice is already on `slice.debug-root-cause` and has a captured symptom plus reproduction, log, test, diff, runtime-state, or other evidence that exposes a wrong value, branch, or state. Exact trigger: the next useful action is to answer "where did this bad data, control path, or state transition enter or change?" by tracing one observed target backward across boundaries.

Scope boundary: this skill records evidence for one trace target inside a debug/root-cause Slice. Do not use it to create the first reproduction, compare a working example, declare root cause, choose a fix, write a regression test, run a deployment, install packages, mutate source/runtime state, or finalize a result. If the issue is a live incident or operational recovery action, route to the operational variant instead.

## Source Contract

Grounding:

- `capabilities/pipelines/slice-variants/debug-root-cause.pipeline.json` step `slice-data-flow-tracer`
- `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#lightweight-and-debug-slice-variants`
- `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6`
- `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19`
- `docs/architecture/master-plugin-target-architecture-capability-atom-shard-map.html#pipeline.slice.debug_root_cause.data.flow.tracer`

The manifest step invokes this skill and `debug-investigation.subagent.json`, produces the step-owned `traces/data-flow.md` receipt and may update `evidence-log.md`, gates on `data_flow_trace_recorded`, reaches `trace_ready`, and records `record_trace_gap` on failure. The atom row defines the technique as tracing bad value, control, or state backward through component boundaries until the source is found.

Source inputs:

- Active Slice front door and contract: `slice.md`, selected variant, authority limits, parent Scope constraints, and current runtime view if available.
- Required debug evidence: `symptom.md`, `reproduction.md`, and current `evidence-log.md`.
- Trace evidence sources: logs, diagnostics, screenshots, failing or passing test output, diffs, git-history notes, config snapshots, runtime-state reads, API responses, database query output, or other already-authorized observations.
- Optional bounded delegation packet for `debug-investigation.subagent.json`: symptom, reproduction evidence, one trace target, known boundary chain, requested observations, and forbidden action list.

## Operating Procedure

1. Load the active debug context: `slice.md`, `symptom.md`, `reproduction.md`, current `evidence-log.md`, relevant logs/tests/diffs/runtime state, and any parent Scope constraints. If the symptom or reproduction evidence is missing, stop and route back to the earlier debug step.
2. Name one trace target. State the observed bad value, branch, or state; the expected value or behavior; where it was first observed; and the evidence reference that proves the observation. Trace one target per pass.
3. Walk backward boundary by boundary. For each hop, record the component, file/function/event/API/service/store/config boundary, inbound value/state, outbound value/state, transformation or decision, caller/upstream source, and exact evidence path or command output reference.
4. Separate facts from questions. Mark unverified assumptions as questions for `slice-hypothesis-ledger`; do not turn a plausible upstream boundary into root cause without proof.
5. Use only authorized read-only inspection and already-available evidence. If more observation is needed, write the proposed diagnostic, expected proof value, authority required, and stop condition. This skill has no source mutation, deployment, package install, or live-system command authorization.
6. If delegating to `debug-investigation.subagent.json`, pass a bounded packet: symptom, reproduction evidence, trace target, current boundary chain, forbidden mutation list, and requested observations. Fan in the response before adding it to the trace.
7. Stop at the earliest evidenced source boundary, or at the first precise gap where the next upstream source is unavailable. Record the next verification step, then route to hypothesis/root-cause decision only after the trace is reviewable.

## Outputs

Create or update `traces/data-flow.md` and optionally append concise entries to `evidence-log.md`. The trace receipt must include the trace target, starting observation, boundary chain, evidence reference for each hop, first evidenced source boundary or trace gap, blocked inputs if any, and proposed next verification step. A successful receipt contains `gate: data_flow_trace_recorded` and `terminal_state: trace_ready`; a failed receipt contains `record_trace_gap` and cannot claim the success pair.

The output may name root-cause candidates as unproven hypotheses, but it must not declare final root cause, choose or apply a fix, mark verification complete, write result status, promote knowledge, or authorize source/runtime mutation.

Terminal states:

- `trace_ready`: `data_flow_trace_recorded` is satisfied with a named target, evidenced boundary chain, and either the earliest evidenced source boundary or a precise no-hop blocker.
- `record_trace_gap`: the trace has a named target and evidence-backed stopping point, but the next upstream source is unavailable, unsafe, or outside current authority.

## Verification

Check the manifest gate before advancing: `data_flow_trace_recorded` is satisfied only when the trace has a named target, at least one evidenced boundary hop or an explicit no-hop blocker, and a clear source boundary or precise gap. Confirm every hop cites evidence rather than intuition, all assumptions are labeled, and the handoff target is clear.

For Layer 6B validation, this body must keep exactly the required sections, reference the owning architecture and manifest paths, avoid wrapper-only boilerplate, include concrete tracing steps, and preserve the no-fix/no-mutation boundary.

## Failure Modes

Block or hand off when the symptom is too vague, reproduction evidence is absent, required logs/tests/runtime state are unavailable, the next boundary requires credentials or unapproved live access, inspection would mutate workspace/runtime state, or the work has become incident response, deployment recovery, or redesign.

Record `record_trace_gap` when the current evidence proves where the trace stops but not where the bad value/control/state originated. Do not fill gaps with guesses, stack fixes, declare root cause from correlation, or continue into fix work without the later debug gates.

When the needed upstream evidence is unavailable or unsafe to inspect, treat the trace as a zero-authority boundary and hand off the exact missing source, owner, and question.

Forbidden actions:

- Do not edit source code, tests, fixtures, config, seed data, generated runtime state, dependency manifests, lockfiles, branches, worktrees, package installs, service state, deployed systems, or live data.
- Do not run commands whose purpose is deployment, package installation, live-system mutation, data repair, queue replay, cache invalidation, migration, rollback, or service restart.
- Do not treat a proposed diagnostic as approved action. If tracing requires mutation or live access, stop with `record_trace_gap` or hand off to the correct authority-bearing pipeline.
