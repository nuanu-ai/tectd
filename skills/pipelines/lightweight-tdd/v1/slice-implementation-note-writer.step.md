---
id: "slice-implementation-note-writer"
kind: "internal_step"
owner_family: "slice_variant"
pipeline_id: "slice.lightweight-tdd-development"
step_id: "slice-implementation-note-writer"
entry_gate: false
public_trigger: false
native_skill_discovery: false
body_path: "capabilities/instructions/pipelines/slice-variants/lightweight-tdd/slice-implementation-note-writer.step.md"
source_manifest: "capabilities/pipelines/slice-variants/lightweight-tdd.pipeline.json"
legacy_skill_ref: "slice-implementation-note-writer"
architecture_refs:
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#lightweight-development"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19"
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#lightweight-and-debug-slice-variants"
---

# Slice Implementation Note Writer

## Overview
This skill records the implementation delta for a lightweight TDD Slice after red-green-refactor work has changed local source, config, docs, generated artifacts, or tests. It is not a final result writer and not a generic summary: it preserves exact implementation decisions and provenance so verification can test the real delta without reconstructing it from chat or memory.

## When to Use
Use this after `slice-tdd-cycle-runner` reaches `implemented_locally` and before `slice-lightweight-verification-runner` starts. Select it for small, bounded lightweight Slice work where acceptance behavior is clear, the affected surface is known, and the Slice needs `implementation-notes.md` to satisfy `implementation_delta_recorded`.

Do not use it to perform implementation, choose tests, run verification, decide deployment impact, write the final result, close the Slice, promote knowledge, or make durable claims. Do not use it for debug/root-cause work, full design-to-execution work, hybrid implementation plus operations, operational execution, or any case that already triggered escalation through ambiguity, unknown cause, live risk, deploy risk, missing test target, or repeated failure.

## Source Contract
Ground this skill in `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#lightweight-development`, `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#lightweight-and-debug-slice-variants`, `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6`, and `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19`. The owning manifest is `capabilities/pipelines/slice-variants/lightweight-tdd.pipeline.json`, step `slice-implementation-note-writer`, which produces `implementation-notes.md`, gates on `implementation_delta_recorded`, blocks on `block_missing_execution_record`, and ends at `implementation_recorded`.

The relevant atom/manifest anchors are `pipeline.slice.lightweight-tdd` and `capabilities/pipelines/slice-variants/lightweight-tdd.pipeline.json#step_graph.steps.slice-implementation-note-writer`. External references are absent for this skill.

## Operating Procedure
1. Confirm the Slice is still on the lightweight path: small understood change, clear acceptance checks, bounded affected surface, no deploy/live requirement, and no repeated-failure or root-cause uncertainty signal. If that is no longer true, stop and route to the appropriate escalation target instead of writing a misleading note.
2. Load only source inputs needed for the note: `slice.md`, `tdd-notes.md` or `test-plan.md`, implementation-step notes, immediate file/git delta, generated-output provenance, and any user-approved scope constraints. Use read-only inspection for evidence gathering.
3. Write `implementation-notes.md` with these required slots in this order: source/provenance links, implementation decisions, touched-file inventory, behavior delta, test delta, deviations from plan, unresolved proof, residual risks, and next routing.
4. Tie every touched file or artifact to a reason. Separate source changes, test changes, docs/config changes, generated artifacts, and deliberately untouched files that influenced the implementation decision.
5. Record TDD continuity without claiming verification: RED failure observed, GREEN change made, refactor-only edits, tests or checks changed, and checks intentionally left for `slice-lightweight-verification-runner`.
6. Record deviations exactly: requirements narrowed or expanded, files changed outside the first target, generated output accepted or ignored, manual edits after tooling, assumptions carried forward, and any evidence that was unavailable.
7. Preserve gaps plainly. Missing execution evidence, unclear file provenance, unreviewed generated output, unexpected side effects, skipped tests, stale source, or unverified behavior must be listed as unresolved proof or residual risk, not smoothed into a completion claim.
8. State next-step routing without performing it: focused/affected verification, deploy-impact check if deployment risk is newly suspected, result writing after proof, promotion routing only after result, or escalation to full/debug/hybrid if lightweight is no longer valid.
9. Stop when the note satisfies `implementation_delta_recorded` and supports the terminal state `implementation_recorded`. Do not continue into verification, deployment, result closure, promotion, or durable knowledge updates.

## Outputs
The output is `implementation-notes.md` in the selected lightweight Slice artifact set. It must include:

- `Source/provenance`: Slice contract, TDD/test-plan source, implementation evidence, file-delta source, and generated-artifact source if any.
- `Implementation decisions`: exact choices made and why.
- `Touched files`: file path, change class, reason, and provenance for source, tests, docs/config, generated artifacts, and intentionally untouched relevant files.
- `Behavior delta`: what behavior was intentionally changed and what was left unchanged.
- `Test delta`: tests added, changed, removed, or intentionally left to verification.
- `Deviations and assumptions`: differences from plan plus assumptions carried forward.
- `Unresolved proof and residual risks`: missing evidence, unverified behavior, skipped checks, or authority gaps.
- `Next routing`: verification, deploy-impact check, result writer, promotion router, handoff, or escalation target.

This artifact may link to evidence already owned by the Slice, but it does not create `verification.md`, `result.md`, `deployment-validation.md`, `promotion.md`, `deferred.md`, `handoff.md`, durable knowledge, or completion records.

## Verification
Check that `implementation-notes.md` exists, belongs to the selected lightweight Slice, references the Slice contract and TDD/test-plan source, and covers every required output slot. Confirm each touched file has a reason and provenance, every implementation decision is tied to evidence, deviations are explicit, unresolved proof is not hidden, and residual risks are carried forward. Confirm the note gives `slice-lightweight-verification-runner` enough detail to choose focused and affected proof.

The proof gate is only `implementation_delta_recorded`. The terminal state is only `implementation_recorded`. The content must not claim verification passed, deployment safety, live behavior, result completion, promotion readiness, durable knowledge promotion, or final Slice closure.

## Failure Modes
Block with `block_missing_execution_record` when no reliable implementation delta exists, changed files cannot be identified, provenance is missing, or the note would depend on memory instead of evidence. Escalate out of lightweight TDD when the delta reveals architecture ambiguity, cross-component uncertainty, unknown root cause, deploy/live validation need, missing test target, repeated failures, or authority risk.

If blocked, hand off only the missing evidence, affected files, current Slice artifact path, and next required skill. If the implementation appears complete but proof is absent, write unresolved proof and route to verification rather than result writing. Use a zero-evidence blocker label only after the missing source is named.
