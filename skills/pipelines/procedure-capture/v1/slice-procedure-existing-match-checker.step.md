---
id: "slice-procedure-existing-match-checker"
kind: "internal_step"
owner_family: "slice_variant"
pipeline_id: "slice.custom-procedure-capture"
step_id: "slice-procedure-existing-match-checker"
entry_gate: false
public_trigger: false
native_skill_discovery: false
body_path: "capabilities/instructions/pipelines/slice-variants/procedure-capture/slice-procedure-existing-match-checker.step.md"
source_manifest: "capabilities/pipelines/slice-variants/procedure-capture.pipeline.json"
legacy_skill_ref: "slice-procedure-existing-match-checker"
architecture_refs:
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#procedure-and-research-slice-variants"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19"
---

# Procedure Existing Match Checker

## Overview

This skill decides whether a captured procedure is already covered by an existing durable artifact. Its job is anti-stockpile classification: detect exact, partial, stale, superseded, conflicting, or absent coverage before the Slice proposes another runbook, procedure, command recipe, KB page, or skill candidate.

Scope boundary: this skill writes only `existing-match-check.md` inside the active procedure-capture Slice. It does not mutate an existing artifact, approve promotion, normalize captured steps, create a proposal, execute a runbook, create an active skill, or write result truth.

Classification: `skill_body`. No external skill body is adapted by this step.

## When to Use

Use this in `slice.custom-procedure-capture` after `source-event.md`, `captured-steps.md`, and `normalized-procedure.md` exist and before durable target selection, authority-risk classification, proposal writing, promotion, result writing, or skill-candidate routing.

Use it when a captured workflow might overlap an existing runbook, procedure note, KB page, protocol or operations record, command recipe, proof-order template, prior procedure proposal, deprecated artifact, superseded artifact, or skill-candidate record. It is also required when the user asks to turn a normalized captured workflow into a durable artifact and the procedure family may already exist.

Do not use it for procedure-capture entry gating, source-event extraction, step normalization, durable target selection, proposal writing, promotion approval, result truth writing, durable-domain mutation, existing runbook execution, deployment, rollback, or live-system action.

## Source Contract

Ground this skill in `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#procedure-and-research-slice-variants`, especially the hybrid/procedure section that requires `existing-match-check.md` and forbids duplicate procedures when an existing one should be improved. Also ground it in `docs/architecture/master-plugin-target-architecture-part-6c-stateful-domain-pipelines.html` for the durable-domain boundary: runbook-library owns reusable procedures, command recipes, proof-order templates, freshness cycles, deprecations, and supersession; a Slice may propose or hand off, but it does not own canonical durable storage. Use `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6` for the procedure-capture Slice role and `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19` for selection and no-auto-promotion routing.

The owning manifest is `capabilities/pipelines/slice-variants/procedure-capture.pipeline.json`, step `slice-procedure-existing-match-checker`. The manifest marks this required validator step as invoking `skill:slice-procedure-existing-match-checker` and `skill:tect-query`, producing `existing-match-check.md`, gating on `existing_match_checked`, failing by `stop_or_handoff`, and continuing only to `ready_for_next_step`.

Read-only inputs are the active procedure-capture packet, `source-event.md`, `captured-steps.md`, `normalized-procedure.md`, source/proof links, known durable-domain references, existing indexes, prior proposals, and query results from `tect-query` or direct read-side inspection. This skill may recommend reuse, update, merge, refresh, supersession review, deprecation review, conflict review, or new-candidate continuation, but it must not perform any of those downstream actions.

## Operating Procedure

1. Verify prerequisites. Require the active procedure-capture Slice, source event, captured steps, normalized procedure shape, source/proof refs, and any known durable target hint. If normalization is missing or the request is still entry gating or extraction, return `stop_or_handoff` to the prior procedure-capture owner.
2. Build the candidate signature. Record purpose, trigger, target system or domain, actor, inputs, preconditions, ordered command/action sequence, expected proof, rollback or failure handling, authority class, secret-risk posture, terminal outcome, and durable target hints. Keep environment-specific values separate from the reusable procedure shape.
3. Search existing durable surfaces read-only. Use `tect-query` or direct indexes to search runbooks, procedure notes, KB pages, protocol records, operations records, command recipes, proof-order templates, prior procedure proposals, deprecated artifacts, superseded artifacts, and skill-candidate records.
4. Use multiple search angles. Search exact titles, aliases, target systems, source repo or service names, action verbs, command names, proof shape, failure mode, authority class, safety gate, stop condition, rollback pattern, domain owner, and parent Program/Scope/Slice references. Record negative search terms and unavailable locations.
5. Compare each candidate against the signature. Check same-purpose overlap, trigger fit, scope boundary, preconditions, ordered steps, command parameters, proof order, authority/risk labels, secret constraints, failure handling, terminal state, freshness, provenance, and whether the candidate is deprecated or superseded by a newer source.
6. Classify one primary verdict: `exact_match`, `partial_match`, `stale_or_superseded_match`, `conflict`, or `no_match`. Use `exact_match` when a current artifact covers the captured procedure without material gaps. Use `partial_match` when updating, merging, or adding a section is better than a new artifact. Use `stale_or_superseded_match` when coverage exists but needs refresh, supersession, or deprecation review. Use `conflict` when sources disagree about safe or correct behavior. Use `no_match` only after bounded searches and negative findings are recorded.
7. Route the anti-stockpile outcome. For `exact_match`, block new durable artifact creation and route to reuse, index/front-door repair, or handoff. For `partial_match`, route to update or merge. For `stale_or_superseded_match`, route to refresh, supersession, or deprecation review. For `conflict`, stop for owner or human review. For `no_match`, allow continuation to durable target selection as a new candidate.
8. Write `existing-match-check.md`. Include the candidate signature, search scope, search terms, durable surfaces inspected, unavailable surfaces, inspected candidate artifacts, negative findings, comparison matrix, duplicate/stale/superseded/conflict analysis, primary classification, confidence, recommended next owner, gate `existing_match_checked`, and terminal state `ready_for_next_step` or `stop_or_handoff`.

## Outputs

The only required artifact write is `existing-match-check.md`. Its output shape must include:

- `classification`: one of `exact_match`, `partial_match`, `stale_or_superseded_match`, `conflict`, or `no_match`.
- `candidate signature`: purpose, trigger, domain, actor, inputs, preconditions, ordered steps, proof, authority, safety, failure handling, and terminal outcome.
- `search record`: read-only sources, search angles, terms, inspected artifacts, unavailable locations, and negative findings.
- `match matrix`: candidate artifact, coverage fit, gaps, freshness, supersession/deprecation status, conflict notes, and evidence links.
- `route`: reuse, update, merge, refresh, supersession review, deprecation review, conflict review, index/front-door repair, handoff, or continuation to durable target selection.
- `gate and terminal`: `existing_match_checked` plus `ready_for_next_step` when continuation is allowed, or `stop_or_handoff` when duplicate, stale, unsafe, conflicting, or insufficient evidence blocks continuation.

This skill must not create `procedure-proposal.md`, `runbook-draft.md`, `command-recipe.md`, `skill-candidate.md`, `promotion-gate.md`, `result.md`, transition records, durable-domain edits, index updates, or source mutations.

## Verification

For source verification, run `node tools/validate-internal-skill-body-quality.mjs --skill slice-procedure-existing-match-checker` and `node tools/validate-internal-skill-trigger-fixtures.mjs --skill slice-procedure-existing-match-checker`. Parse both owned fixture JSON files, confirm the skill body still has exactly the seven Layer 6B H2 sections, run scoped `git diff --check` on this skill and its two fixture files, and scan the three owned files for trailing whitespace.

For behavior verification, inspect `existing-match-check.md`. It must show the normalized procedure signature, read-only search scope, search angles, candidate artifacts, negative findings, evidence-backed comparison, one primary classification, an anti-stockpile route, and a safe terminal state. Verify that `exact_match`, `partial_match`, `stale_or_superseded_match`, and `conflict` do not silently create another durable artifact, while `no_match` is supported by bounded negative findings.

## Failure Modes

Return `stop_or_handoff` when required procedure inputs are missing, existing durable surfaces cannot be read, the search scope is too narrow to support a verdict, relevant artifacts are private or unsafe to quote, evidence links are stale, authority or secret-risk constraints make comparison unsafe, or candidate artifacts conflict about safe procedure behavior.

Block or hand off instead of creating another artifact when an exact match already exists, a partial match should be updated, a stale artifact needs refresh, a superseded artifact needs reconciliation, deprecated guidance needs owner review, or duplicate status depends on a human owner. Preserve candidate lists and negative findings so maintenance or the durable-domain owner can reconcile, retire, refresh, or promote the right artifact later.

Do not continue in this skill when the request belongs to entry gating, event extraction, step normalization, target selection, proposal writing, promotion approval, result truth writing, active skill creation, durable runbook mutation, procedure execution, deployment, rollback, or live-system action.
