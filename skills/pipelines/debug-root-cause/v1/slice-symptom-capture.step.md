---
id: "slice-symptom-capture"
kind: "internal_step"
owner_family: "slice_variant"
pipeline_id: "slice.debug-root-cause"
step_id: "slice-symptom-capture"
entry_gate: false
public_trigger: false
native_skill_discovery: false
body_path: "capabilities/instructions/pipelines/slice-variants/debug-root-cause/slice-symptom-capture.step.md"
source_manifest: "capabilities/pipelines/slice-variants/debug-root-cause.pipeline.json"
legacy_skill_ref: "slice-symptom-capture"
architecture_refs:
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#debug-root-cause-variant"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19"
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#lightweight-and-debug-slice-variants"
---

# Slice Symptom Capture

## Overview

This skill creates the first symptom artifact for `slice.debug-root-cause`. Its only job is to capture observable symptom truth: what should have happened, what actually happened, where and when it happened, what evidence currently supports that report, and what is still unknown.

Stop at symptom capture. A captured symptom is not a reproduction, hypothesis, diagnosis, root-cause decision, fix plan, verification result, promotion record, or durable knowledge claim.

## When to Use

Use this skill only after the debug entry, context, and contract steps have selected `slice.debug-root-cause`, root cause is unknown, and the next required artifact is `symptom.md`.

Select it when a bug, regression, failed check, unexpected runtime behavior, build failure, integration failure, contradictory evidence, or user report contains enough observable detail to start a debug/root-cause Slice but lacks a formal symptom record.

Do not select it when:

- `symptom.md` already records the expected behavior, observed behavior, affected surface, environment, timestamp/source, and explicit delta.
- The next needed step is reproduction, evidence ordering, recent-change inspection, data-flow tracing, hypothesis tracking, working-example comparison, root-cause decision, fix strategy, fix execution, verification, result writing, promotion, or maintenance cleanup.
- The cause is already known and the work is a narrow accepted implementation change; route to lightweight TDD or full Slice selection as appropriate.
- User impact, rollback, deploy authority, incident mitigation, or live operational execution dominates; route to operational or hybrid handling.
- The request is only a current-state query, stale artifact check, result lookup, or durable knowledge question.

## Source Contract

Ground this step in:

- `capabilities/pipelines/slice-variants/debug-root-cause.pipeline.json#step_graph.steps.slice-symptom-capture`
- `capabilities/pipelines/slice-variants/debug-root-cause.pipeline.json#step_graph.steps.slice-symptom-capture.invokes.slice-symptom-capture`
- `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#debug-root-cause-variant`
- `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#lightweight-and-debug-slice-variants`
- `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6`
- `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19`
- `docs/architecture/master-plugin-target-architecture-capability-atom-shard-map.html#pipeline.slice.debug_root_cause.symptom.capture`

The manifest step is required, invokes `slice-symptom-capture`, produces only `symptom.md`, gates on `observed_expected_delta_recorded`, reaches `symptom_recorded`, and fails through `block_missing_symptom`.

Read these source inputs before writing the symptom record when they exist: active Slice front door or debug contract, user report, pasted failure text, failing command or check name, affected file/module/API/screen/job/service, environment name, timestamp or time window, branch/commit/version/config, prior evidence pointers, authority constraints, and parent Scope constraints. If an input is missing, record it as unknown instead of inventing it.

## Operating Procedure

1. Confirm routing. The active work must be a Slice under `slice.debug-root-cause`, not lightweight, full design-to-execution, operational, hybrid, research, procedure capture, query, maintenance, setup, or team integration.
2. Confirm sequence. Entry gate, context loader, and debug contract should already establish root cause unknown, allowed actions, proof order posture, and that `symptom.md` is the next missing artifact. If those are absent, route back to the relevant earlier debug step.
3. Capture source facts without expanding the investigation. Note who or what reported the symptom, the report timestamp or time window, evidence freshness, and whether the evidence is current, pasted, historical, memory-derived, intermittent, environment-specific, or contradicted.
4. Run the required question sweep:
   - What exact behavior was expected?
   - What exact behavior was observed?
   - What is the explicit observed-versus-expected delta?
   - What affected surface showed the behavior: command, test, API, UI, job, service, module, chain, config, or artifact?
   - What environment, branch, commit, version, input, account, dataset, or configuration was involved?
   - When was it observed, and by what source?
   - What evidence pointers exist now, such as error text, logs, screenshots, traces, test names, request IDs, CI URLs, or pasted output?
   - What authority and safety constraints limit the next debug steps?
   - What is still missing before reproduction or evidence planning can begin?
5. Write expected and observed behavior as separate, checkable statements. Include exact values, status codes, messages, counts, assertions, IDs, versions, paths, flags, and time windows when available.
6. Name the delta in one sentence. The delta must be stronger than "broken", "fails", or "does not work"; it must compare the expected statement to the observed statement.
7. Record constraints and non-routes. Preserve missing logs, unavailable environments, credentials, sensitive material, live-impact risk, deploy risk, and any reason this normal debug path must hand off to ops, hybrid, full Slice, or a blocker.
8. Do not run tests, execute reproduction commands, query live systems, inspect git history for causes, add instrumentation, edit source, change config, deploy, write a fix plan, assert root cause, write verification, update result truth, promote knowledge, or perform maintenance cleanup from this step.
9. Apply the proof gate. Use `symptom_recorded` only when `observed_expected_delta_recorded` is satisfied by concrete expected behavior, observed behavior, affected surface, environment, source/time, and uncertainty fields. Otherwise use `block_missing_symptom` and ask for the smallest missing fact.

## Outputs

Produce `symptom.md` or update the active debug Slice symptom section with this shape:

```markdown
# Symptom

- Slice: <id or path>
- Pipeline variant: slice.debug-root-cause
- Manifest step: slice-symptom-capture
- Source report: <user, command output, CI, log, screenshot, trace, pasted text, or other source>
- Source freshness: <current | pasted | historical | memory-derived | intermittent | contradicted | unknown>
- Observed at: <timestamp, time window, commit, version, environment, or unknown>
- Affected surface: <API, screen, test, command, module, job, service, config, artifact, or other surface>
- Expected behavior: <checkable statement>
- Observed behavior: <checkable statement>
- Observed-versus-expected delta: <one concrete comparison sentence>
- Evidence pointers available now: <paths, IDs, snippets, links, names, or none recorded>
- Authority and safety constraints: <allowed action boundary and blocked side effects>
- Missing information: <smallest missing facts>
- Rejected or deferred routes: <lightweight, ops, hybrid, full, query, maintenance, or none>
- Gate: observed_expected_delta_recorded=<yes | no>
- Terminal state: <symptom_recorded | block_missing_symptom>
- Next route: <slice-reproduction-builder | earlier debug gate | ops/hybrid/full/query/maintenance handoff | ask user>
```

The output is a handoff input for reproduction and evidence-order steps. It must not include completed reproduction proof, accepted hypotheses, root-cause claims, source patches, deployment notes, verification verdicts, result closure, promotion decisions, or durable knowledge claims.

## Verification

Before leaving this step, verify the record answers the question: "What observable behavior differed from what expected behavior, on which surface, in which environment, at what time or source, with what evidence pointer, under what authority boundary?"

The proof gate passes only when `observed_expected_delta_recorded` is explicit and independently readable. A future agent must be able to identify the symptom without chat history and must know what remains unknown before reproduction. If the record relies on a vague label, inferred cause, workaround, suspicion, or unstated environment, the gate fails.

Also verify the side-effect boundary: this step produced only `symptom.md` or its symptom section. It did not run the debug pipeline, persist active pipeline state, mutate a source repo, mutate a worktree or branch, execute package/install/deploy/live-system commands, write results, perform promotion, or run maintenance.

## Failure Modes

Use `block_missing_symptom` when expected behavior, observed behavior, affected surface, environment, timestamp/source, evidence pointer, or authority boundary is too vague to support the next debug step. Ask for the smallest missing fact, such as "which command failed", "what value was expected", "what value was observed", "which environment", or "when was this seen".

Route away instead of forcing symptom capture when the selected variant is wrong, the cause is already known, the next artifact is later than `symptom.md`, live incident authority dominates, the user only asked for current-state truth, or privacy/safety restrictions prevent recording enough observable detail.

Do not let a stack trace, failing assertion, log line, workaround, suspected commit, or user theory become a root-cause claim. Do not use missing symptom detail as permission to inspect broadly, run tests, add logging, patch code, deploy, write a result, or promote knowledge. A zero-detail report is a blocker, not a license to guess.
