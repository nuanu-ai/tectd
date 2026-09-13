---
id: "slice-procedure-event-extractor"
kind: "internal_step"
owner_family: "slice_variant"
pipeline_id: "slice.custom-procedure-capture"
step_id: "slice-procedure-event-extractor"
entry_gate: false
public_trigger: false
native_skill_discovery: false
body_path: "capabilities/instructions/pipelines/slice-variants/procedure-capture/slice-procedure-event-extractor.step.md"
source_manifest: "capabilities/pipelines/slice-variants/procedure-capture.pipeline.json"
legacy_skill_ref: "slice-procedure-event-extractor"
architecture_refs:
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#hybrid-implementation-operation-and-procedure-capture-variants"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19"
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#procedure-and-research-slice-variants"
---

# Slice Procedure Event Extractor

## Overview

This is a standalone executable skill body for the procedure-capture source-event step. It extracts a faithful event trace from an unexpected/custom operation transcript or linked user-agent workflow before any later step turns the material into reusable steps, procedure text, runbook text, proof templates, durable knowledge, or a skill candidate.

The core rule is source fidelity: preserve what actually happened, who acted, what was observed, which decisions changed the path, what hazards appeared, what proof closed or blocked the event, and what source gaps remain. This skill does not decide that the procedure is accepted, reusable, safe, or promotable.

## When to Use

Trigger/non-trigger boundary:

- Use this after `slice-procedure-source-context-loader` has linked or declared source context for `slice.custom-procedure-capture`.
- Use this when the next question is exactly: what triggered the event, who acted, which commands/actions occurred, what was observed, which decisions changed the path, what hazards appeared, which proof closed or blocked the event, which variants were tried, and what data is still missing?
- Use this for unexpected deploy verification, incident workaround, debug proof-order discovery, live-operation recovery, repeated command sequence, custom user-agent workflow, or another ad hoc operation that may later become a procedure/runbook candidate.

Do not use this when the work is procedure-capture entry gating, source-context loading, step normalization, procedure generalization, existing-match checking, secret scrubbing, authority/risk classification, proof-contract design, reuse-fit evaluation, proposal writing, promotion approval, result writing, skill authoring, execution of a runbook, or direct query answering.

## Source Contract

Ground this step in `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#hybrid-implementation-operation-and-procedure-capture-variants`, `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#procedure-and-research-slice-variants`, `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6`, `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19`, and `docs/architecture/master-plugin-target-architecture-capability-atom-shard-map.html#pipeline.slice.procedure-capture`.

The owning manifest is `capabilities/pipelines/slice-variants/procedure-capture.pipeline.json`. The exact step is `step_graph.steps.slice-procedure-event-extractor`; it is required, invokes this skill, records `source-event.md`, advances gate `source_event_extracted`, reaches `ready_for_next_step`, and uses `stop_or_handoff` on failure.

Source inputs should already include a source Slice or session event, source-context packet, transcript or session evidence, command history, tool calls, logs, artifacts, source links, user decisions, authority state, secret-risk posture, and durable-domain read-side context when available. External references: none for this step.

The side-effect boundary is read/trace only. The skill may prepare `source-event.md` content for the active procedure-capture Slice, but it must not execute commands, mutate durable libraries, edit canonical runbooks, create skills, update registries, run deployment actions, persist active pipeline state, or touch source repos/worktrees.

## Operating Procedure

Extraction procedure:

1. Confirm the selected variant is `slice.custom-procedure-capture`, the parent Slice or Result is known, and the source-context step has either linked sources or declared gaps. If not, stop and route to `slice-procedure-capture-entry-gate` or `slice-procedure-source-context-loader`.
2. Identify the event trigger in concrete terms: user request, agent observation, failure, workaround, live-operation need, repeated workflow signal, or unexpected useful method that made procedure capture relevant.
3. List actors and roles without simplifying them away: user, agent, subagent, local tool, remote service, reviewer, CI/deploy system, live system, or blocked human owner. Mark unknown actors as missing data.
4. Build a chronological trace from evidence, not memory: commands, tool calls, file reads, edits, checks, approvals, observations, branch/workspace state, logs, screenshots, external links, and user decisions. Preserve exact command/action text only when safe; otherwise record a redacted placeholder and custody note for the later secret-safety step.
5. For each event, capture why it mattered: decision made, proof obtained, hazard found, variant tried, stop condition hit, or reason the path changed. Preserve failed attempts and rejected variants when they explain the eventual workflow.
6. Separate observations from decisions. Observations are evidence-backed facts such as outputs, errors, diffs, test results, deploy state, or user-visible behavior. Decisions are route choices, authority calls, risk downgrades, handoffs, or "do not proceed" calls.
7. Mark hazards and boundaries as trace facts: secret exposure risk, live-system risk, destructive command risk, stale source risk, duplicate-runbook risk, authority gaps, missing proof, unverified assumptions, and environment-specific details that later steps must not copy blindly.
8. Record proof gates at the source-event level: evidence cited for trigger, actors, chronological actions, decisions, hazards, tried variants, final proof, and missing data. Do not upgrade weak proof into durable truth.
9. End with terminal/failure states and missing data needed by later steps: absent logs, unclear timestamps, incomplete command output, unknown actor, unavailable artifact, unsourced decision, unverified hazard, source-access blocker, or evidence requiring user confirmation.

## Outputs

Output shape: return or write only the event trace content for `source-event.md`, depending on the caller's artifact-write authority. It should contain: event trigger, source inputs used, actors, chronological commands/actions, observations, decisions, hazards, proof references, tried variants, missing data, source links, and terminal event status.

Valid terminal status for this step is `ready_for_next_step` only when the trace is specific enough for step normalization. If source evidence is insufficient, use `stop_or_handoff` with a concrete blocker such as `blocked_missing_source_event`, `blocked_source_access`, `blocked_secret_risk`, or `blocked_unsourced_decision`.

The output must remain a source-event trace. It must not become `captured-steps.md`, `normalized-procedure.md`, `procedure-proposal.md`, `promotion-gate.md`, a durable runbook, a knowledge-base page, or a skill implementation. It must not claim procedure accepted, reusable, promoted, published, or safe for future execution.

Forbidden actions: no normalization into captured steps, no runbook generalization, no duplicate verdict, no secret scrub proof, no proposal or promotion decision, no command execution, no deployment, no live-system mutation, no durable-domain write, no active skill creation, no registry/status/ledger mutation, and no claim that this step completed the full procedure-capture Slice.

## Verification

Validation starts with the artifact itself. Check that `source-event.md` answers the event questions with evidence-backed entries: what triggered the workflow, who acted, what commands/actions occurred, what was observed, which decisions redirected the path, what hazards existed, what proof closed or blocked the event, which variants were attempted, and what remains missing.

Verify proof gates before setting `source_event_extracted`: every key event has a source reference or explicit gap, observations and decisions are separated, final proof is classified without inflation, missing proof blocks readiness, and uncertain, stale, secret-bearing, or contradictory material is preserved for later checks instead of hidden.

Run scoped validation for this skill body and trigger fixture before calling the step ready: `node tools/validate-internal-skill-body-quality.mjs --skill slice-procedure-event-extractor` and `node tools/validate-internal-skill-trigger-fixtures.mjs --skill slice-procedure-event-extractor`.

## Failure Modes

Use terminal/failure states literally. Return `stop_or_handoff` when the source event cannot be located, the context packet points only to vague memory, command or proof evidence is missing, actor/authority ownership is unclear, the trace depends on raw secrets or unsafe private material, source access is blocked, or the available source is too contradictory to reconstruct a faithful event.

Handoff/escalation routes:

- Route to `slice-procedure-source-context-loader` for missing source links, absent transcripts, unavailable logs, or vague memory-only context.
- Route to `slice-procedure-step-normalizer` only after `source-event.md` is specific enough to feed ordered reusable steps.
- Route to `slice-procedure-secret-safety-scrubber`, `slice-procedure-authority-and-risk-classifier`, `slice-procedure-existing-match-checker`, or `slice-procedure-proof-contract-builder` when the blocker belongs to those later checks.
- Route to runbook-library, durable-knowledge, or skill-authoring workflows only through later proposal, validation, promotion, and result steps; this skill must not mutate durable libraries or create active skills.
- Route away entirely when the user is asking to execute an existing runbook, operate a live system, deploy, edit a canonical runbook, promote the candidate, or claim the procedure is accepted.
