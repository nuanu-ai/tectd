---
id: "slice-research-synthesis-builder"
kind: "internal_step"
owner_family: "slice_variant"
pipeline_id: "slice.research-to-durable-knowledge"
step_id: "slice-research-synthesis-builder"
entry_gate: false
public_trigger: false
native_skill_discovery: false
body_path: "capabilities/instructions/pipelines/slice-variants/research-to-durable-knowledge/slice-research-synthesis-builder.step.md"
source_manifest: "capabilities/pipelines/slice-variants/research-to-durable-knowledge.pipeline.json"
legacy_skill_ref: "slice-research-synthesis-builder"
architecture_refs:
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#research-to-durable-kb-slice-variant"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19"
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#procedure-and-research-slice-variants"
---

# Slice Research Synthesis Builder

## Overview

This skill turns an already-ledgered Research To Durable Knowledge Slice into slice-local `synthesis.md`. It is a fan-in step: build decision-ready findings, patterns, implications, open questions, promotion posture, and KB seed inputs from reviewed claim IDs.

The boundary is strict. The synthesis is not durable truth, not a promotion approval, and not a canonical KB write. It may synthesize already extracted and reconciled claims because the manifest says this step produces `synthesis.md`; it must preserve contradiction, stale, source-only, restricted, negative, blocked, rejected, and deferred findings instead of smoothing them into clean prose.

## When to Use

Use this only when all of these trigger conditions are true:

- the selected variant is `slice.research-to-durable-knowledge`;
- the selected manifest step is `slice-research-synthesis-builder`;
- the next gate is `synthesis_built_from_claim_ledger`;
- upstream artifacts exist or are explicitly gap-recorded: `research-questions.md`, `source-map.md`, `evidence-corpus/index.md`, `evidence-log.md`, `source-provenance.md`, `freshness-authority.md`, `claim-ledger.md`, `contradictions-and-gaps.md`, and `negative-knowledge.md`;
- the task is to produce `synthesis.md` before durable object modeling, `durable-kb-seed.md`, promotion edge writing, promotion gate evaluation, result closure, or durable-domain handoff.

Do not select this skill when the request is raw research collection, evidence ingestion, provenance/freshness labeling, claim extraction, claim ledger construction, contradiction classification, negative knowledge capture, durable object modeling, KB seed proposal writing, promotion approval, canonical durable KB mutation, index/front-door update, procedure capture, debug/root-cause work, or a direct simple current-state answer.

If prerequisites are missing, stop or route to the upstream owner rather than fabricating synthesis. If the user asks for immediate durable truth, route to the relevant promotion or stateful-domain pipeline owner.

## Source Contract

Grounding sources and anchors:

- `capabilities/pipelines/slice-variants/research-to-durable-knowledge.pipeline.json`, step `slice-research-synthesis-builder`, produces `synthesis.md`, terminal state `ready_for_next_step`.
- `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#research-to-durable-kb-slice-variant`.
- `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#procedure-and-research-slice-variants`.
- `docs/architecture/master-plugin-target-architecture-part-6c-stateful-domain-pipelines.html#durable-kb-pipeline`, which keeps canonical durable knowledge behind authority, freshness, contradiction, provenance, and index/front-door gates.
- `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6` and `#s19`.
- `docs/architecture/master-plugin-target-architecture-capability-atom-shard-map.html` anchors `pipeline.slice.research-to-durable-knowledge` and `pipeline.slice.research_to_durable_knowledge.synthesis.builder`.

The manifest contract for this step is required, invokes `skill:slice-research-synthesis-builder`, produces only `synthesis.md`, gates on `synthesis_built_from_claim_ledger`, fails by `stop_or_handoff`, and reaches step terminal state `ready_for_next_step`.

Source inputs are the Slice parent contract, research questions, source map, evidence corpus index, evidence log, provenance labels, freshness and authority labels, claim ledger, contradiction and gap review, negative knowledge, optional comparison matrix, optional subagent reports, and any existing durable KB context loaded by prior steps. Treat them as source truth for synthesis only, not as permission to refresh sources or promote claims.

## Operating Procedure

1. Confirm the active packet names `slice.research-to-durable-knowledge`, step `slice-research-synthesis-builder`, and gate `synthesis_built_from_claim_ledger`. If Runtime did not select this step, do not run it.
2. Verify the required source inputs exist or are explicitly gap-recorded. Required inputs are `source-provenance.md`, `freshness-authority.md`, `claim-ledger.md`, `contradictions-and-gaps.md`, and `negative-knowledge.md`; use `research-questions.md`, `source-map.md`, `evidence-log.md`, and `evidence-corpus/index.md` to audit traceability.
3. Audit `claim-ledger.md` without adding new claims. Group claim IDs by research question, topic, target durable-lane hint, source role, evidence class, confidence, freshness, authority, restriction, contradiction status, gap handle, negative-knowledge handle, allowed use, and forbidden-promotion state.
4. Write supported findings only from claim groups. Each finding must cite claim IDs and preserve evidence class, source provenance, freshness/authority labels, confidence, allowed use, and whether the finding is factual support or an inference.
5. Preserve limitations in separate buckets: stale, restricted, source-only, contradicted, missing proof, unresolved gap, rejected, unsafe, non-publishable, deferred, and blocked by authority. Do not resolve contradictions by assertion.
6. Pull negative knowledge forward. State what was disproved, unsupported, out of scope, unsafe to publish, stale, contradicted, or not durable yet, with claim IDs, gap handles, or ledger references.
7. Record promotion readiness as posture, not approval. Allowed posture labels are ready candidate, candidate with restrictions, blocked by missing sources, blocked by freshness or authority, blocked by contradiction, rejected insufficient evidence, handoff for human review, or route to durable-domain pipeline.
8. Prepare KB seed inputs only as structured next-step material: target object hints, candidate section titles, claim IDs, required citations, warnings, restrictions, non-promotable exclusions, unresolved prerequisites, target owner questions, and maintenance/freshness needs. Do not write `durable-kb-seed.md` prose.
9. End with the proof gate. Mark `ready_for_next_step` only when `synthesis.md` can be audited from claim IDs to source/provenance/freshness labels and every blocked, source-only, stale, contradicted, rejected, restricted, or deferred item remains visible.

## Outputs

The only primary artifact is `synthesis.md`.

Required shape:

- input inventory with source artifact paths and any missing/gap-recorded input;
- supported findings grouped by research question or target durable object hint;
- patterns and implications, with inference labels;
- limitation table for stale, restricted, source-only, contradicted, missing-proof, rejected, unsafe, non-publishable, and deferred material;
- contradiction and gap summary;
- negative knowledge carried forward;
- confidence, freshness, authority, and allowed-use summary;
- promotion readiness posture, not promotion approval;
- KB seed inputs for the later durable object modeler and seed proposal writer;
- follow-up questions, handoff owner, and next proof required.

The only successful step terminal state is `ready_for_next_step` after `synthesis_built_from_claim_ledger` is satisfied. Failure or unsafe posture is a blocked/handoff note, not a completed synthesis. Slice-level downstream states that may be referenced as posture are `research_synthesized_not_promoted`, `blocked_by_missing_sources`, `blocked_by_freshness_or_authority`, `blocked_by_contradiction`, `rejected_insufficient_evidence`, `handoff_for_human_review`, and `escalated_to_durable_domain_pipeline`. This step must not claim `durable_kb_seed_proposed`, `promoted_to_durable_kb`, or `promoted_with_restrictions`.

## Verification

Verify every finding cites claim IDs from `claim-ledger.md` and does not introduce new facts. Cross-check those IDs against `source-provenance.md`, `freshness-authority.md`, `contradictions-and-gaps.md`, `negative-knowledge.md`, `evidence-log.md`, and `evidence-corpus/index.md`.

Confirm provenance, freshness, authority, confidence, allowed use, restriction, contradiction, gap, negative knowledge, source-only status, stale status, and forbidden-promotion status survived into `synthesis.md`.

Confirm the output creates no `durable-kb-seed.md`, approves no promotion, edits no canonical durable KB, updates no front-door/index files, refreshes no evidence, executes no researched code, and resolves no contradiction by assertion. The synthesis is acceptable only when a downstream reviewer can decide what may become durable knowledge and what must remain blocked, restricted, rejected, source-only, stale, or deferred.

## Failure Modes

Stop or hand off when required upstream artifacts are missing, claim IDs are absent, sources lack provenance or freshness labels, authority is unresolved, contradictions are unclassified, negative knowledge was skipped, restricted material cannot be summarized safely, privacy limits block citation, or the task needs live refresh before current claims can be made.

Handoff routes:

- missing sources, evidence, or extracted claims: route to evidence collection, evidence ingestion, claim extraction, or claim ledger builder;
- missing provenance, freshness, or authority: route to provenance/freshness labeling or human authority review;
- unclassified contradictions, gaps, or negative findings: route to contradiction/gap classification or negative knowledge capture;
- need for current live truth: stop for refresh or query/freshness route;
- durable object modeling or KB seed writing: route to durable object modeler or KB seed proposal writer;
- canonical durable KB write or index/front-door update: route to the stateful durable KB/domain owner or index/front-door checker;
- procedure capture, debug/root-cause work, or direct simple answer: route to that selected variant or query service.

Forbidden actions: no durable KB mutation, no canonical knowledge write, no index/front-door mutation, no promotion approval, no active pipeline state persistence, no researched code execution, no source refresh hidden inside synthesis, no procedure/runbook capture, no contradiction resolution by assertion, and no hiding of stale, source-only, restricted, contradicted, negative, blocked, rejected, or deferred findings.
