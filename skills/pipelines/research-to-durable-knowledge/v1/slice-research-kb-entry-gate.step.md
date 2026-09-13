---
id: "slice-research-kb-entry-gate"
kind: "internal_step"
owner_family: "slice_variant"
pipeline_id: "slice.research-to-durable-knowledge"
step_id: "slice-research-kb-entry-gate"
entry_gate: true
public_trigger: false
native_skill_discovery: false
body_path: "capabilities/instructions/pipelines/slice-variants/research-to-durable-knowledge/slice-research-kb-entry-gate.step.md"
source_manifest: "capabilities/pipelines/slice-variants/research-to-durable-knowledge.pipeline.json"
legacy_skill_ref: "slice-research-kb-entry-gate"
architecture_refs:
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#research-to-durable-kb-slice-variant"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19"
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#procedure-and-research-slice-variants"
---

# Slice Research KB Entry Gate

## Overview
This skill is the entry gate for `slice.research-to-durable-knowledge`. Its core rule is to admit only proposal-only research work whose question, source boundary, durable lane candidate, authority, and freshness posture are explicit enough to continue without treating research output as durable truth.

## When to Use
Use this skill when Runtime has selected, or is about to select, the Research To Durable Knowledge Slice and the immediate decision is whether `slice-research-kb-entry-gate` can produce `slice.md` with `research_target_declared`.

Trigger examples: substantial research that may later seed a durable KB object, runbook, protocol page, product or operations knowledge page, decision record, issue/follow-up Scope, source corpus, claim ledger, or promotion edge.

Do not use it for a simple cited answer, direct KB query, debug/root-cause investigation, procedure capture, normal implementation planning, source-map construction after entry passed, evidence collection, claim extraction, synthesis, seed writing, promotion, index update, durable-domain pipeline execution, or a direct durable KB write request.

## Source Contract
Grounding sources are `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#research-to-durable-kb-slice-variant`, `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#procedure-and-research-slice-variants`, `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6`, `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19`, and the research row in `docs/architecture/master-plugin-target-architecture-capability-atom-shard-map.html`.

The owning manifest is `capabilities/pipelines/slice-variants/research-to-durable-knowledge.pipeline.json`, step `slice-research-kb-entry-gate`. The step is required, invokes `skill:slice-research-kb-entry-gate`, produces `slice.md`, gates on `research_target_declared`, reaches `ready_for_next_step`, and fails through `stop_or_handoff`.

Registry linkage: `capabilities/registry/internal-skill-resolutions.json`, record `tect-skill.slice-procedure-research.slice-research-kb-entry-gate`, maps the manifest step to this Tect-owned `skill_body`. `capabilities/registry/internal-skill-body-implementation-status.json` and `capabilities/registry/internal-skill-fidelity-status.json` are status evidence only; this gate must still obey the read/propose boundary.

Authority is read/propose only: no durable KB write, no claim promotion, no subagent dispatch, no external research call, no researched code execution, no source import, no workspace mutation, no deployment, and no live-system command starts here.

## Operating Procedure
1. Load the active Kernel/Runtime packet, parent Program/Scope/Slice target, selected or proposed variant, authority state, freshness requirement, target durable knowledge lane candidate, and any prior KB context or context packet.
2. Check route fit. Accept only when the request needs proposal-only research-to-durable-KB handling. Reject or reroute simple answers, KB lookups, debug/root-cause work, procedure capture, implementation planning, downstream research steps, promotion work, and durable-domain writes.
3. Declare the entry contract. Name the research question or brief, source scope, expected decision or reuse value, durable lane candidate, source classes expected, and why output remains proposal-only until evidence, provenance, freshness, contradiction, promotion, and target-owner gates run.
4. Build the input-readiness table. For research questions, source map seed, evidence plan, evidence corpus, claim ledger, provenance labels, freshness/authority labels, contradictions and gaps, negative knowledge, synthesis inputs, and prior KB context, record `present`, `missing_route`, or `blocks_entry`.
5. Label risk before continuation. Mark stale, source-only, generated, memory-derived, restricted, contradictory, untrusted, or safety-limited material so later steps cannot treat it as current durable authority.
6. Route missing inputs to their owners instead of filling gaps: question framing, source-map builder, evidence-plan builder, evidence collector, evidence ingester, provenance/freshness labeler, claim-ledger builder, contradiction-and-gap classifier, negative-knowledge capturer, synthesis builder, or human review.
7. Write `slice.md` with the route-fit verdict, `research_target_declared` value, accepted and rejected routes, input-readiness table, missing-input routes, freshness/authority flags, contradiction and negative-knowledge notes, no-durable-write boundary, next route, and terminal decision.
8. Choose the terminal path. Use `ready_for_next_step` only when the research target and entry inputs are bounded enough for `slice-research-kb-context-loader` or a named missing-input owner. Use `stop_or_handoff` when the gate cannot safely declare the research target.

## Outputs
Primary artifact: `slice.md`.

Required shape:

- `selected_variant`: `slice.research-to-durable-knowledge`.
- `parent_spine_target`: Program, Scope, Slice, or explicit missing parent.
- `route_fit`: accepted or rejected, with rejected alternate routes.
- `research_target_declared`: yes/no plus the research question, source scope, durable lane candidate, authority state, and freshness requirement.
- `input_readiness`: table covering research questions, source map seed, evidence plan, corpus, claim ledger, provenance, freshness, contradictions, negative knowledge, synthesis inputs, and prior KB context.
- `boundary`: no durable KB write, no promotion, no external execution, no hidden durable-domain bypass.
- `next_route`: next skill or alternate owner.
- `terminal_decision`: one of the terminal states below.

Do not create research artifacts, source corpora, claim ledgers, durable KB seed pages, promotion records, index updates, or durable-domain pages from this gate.

## Proof Gates
Pass only when `slice.md` proves all of these:

- Route fit is explicit and rejects simple answer, KB query, debug/root-cause, procedure capture, implementation, downstream research, promotion, and durable-write routes when they are the real request.
- Research target, source scope, durable lane candidate, authority state, and freshness requirement are present or block entry.
- Every required input has `present`, `missing_route`, or `blocks_entry`; no gap is silently assumed.
- Stale, source-only, restricted, contradictory, memory-derived, generated, and untrusted sources are labeled before downstream work.
- The next route is named and stays inside read/propose authority.

## Terminal States
Use `ready_for_next_step` only when `research_target_declared` is true and the next owner is clear.

Use `stop_or_handoff`, `blocked_missing_research_target`, `blocked_missing_source_scope`, `blocked_missing_authority`, `blocked_stale_or_untrusted_sources`, `blocked_by_contradiction`, `blocked_missing_prior_kb_context`, `requires_simple_query`, `requires_debug_variant`, `requires_procedure_capture`, `requires_downstream_research_step`, or `requires_durable_domain_pipeline` when entry cannot proceed safely.

## Failure Modes
Block when the request lacks a research target, source scope, source-map seed, authority state, freshness requirement, target durable lane candidate, prior KB context, or enough source custody to decide entry safely.

Route away when the real task is a simple answer, KB query, debug/root-cause investigation, procedure capture, implementation planning, source-map construction after entry has passed, evidence collection, claim extraction, synthesis, promotion, index/front-door update, or durable-domain write.

Stop or hand off when available material is stale, contradictory, restricted, memory-derived without source custody, untrusted external code, raw evidence that cannot be safely summarized, or a human decision is needed before selecting a durable lane.

## Forbidden Actions
Never perform the research, collect evidence, dispatch subagents, import a source corpus, execute researched code or installers, write KB pages, create durable object pages, update indexes, promote claims, write promotion edges, persist active pipeline state, mutate workspace/source files, deploy, or call live systems from this gate.

Never bypass durable-domain gates. A durable KB seed may be proposed later by `slice-research-kb-seed-proposal-writer`; durable writes belong to the target durable-domain owner after promotion gates and target-owner authority.

## Next Routes
Successful entry routes to `slice-research-kb-context-loader` or, when the gate finds a missing prerequisite, to the exact missing-input owner named in `slice.md`.

Alternate routes are `tect-query` or KB query service for simple answers and current-state lookups, `slice.debug-root-cause` for unknown-cause failures, `slice.custom-procedure-capture` for reusable procedure capture, `slice.full-design-to-execution` or lightweight implementation for code work, and the relevant durable-domain pipeline only when a corpus and promotion package already exist.

## Verification
Verify `slice.md` before handoff:

- H1/H2 shape includes Overview, When to Use, Source Contract, Operating Procedure, Outputs, Proof Gates, Terminal States, Failure Modes, Forbidden Actions, Next Routes, and Verification.
- `research_target_declared`, `ready_for_next_step`, and `stop_or_handoff` are used only according to the manifest step contract.
- The artifact names accepted and rejected routes, required source inputs, missing-input routes, freshness/authority posture, contradiction posture, negative-knowledge posture, next route, and read/propose boundary.
- It makes no durable truth, completion, promotion, execution, subagent, deployment, or durable-write claim.

Fixture checks: run `node tools/validate-internal-skill-body-quality.mjs --skill slice-research-kb-entry-gate` and `node tools/validate-internal-skill-trigger-fixtures.mjs --skill slice-research-kb-entry-gate`.
