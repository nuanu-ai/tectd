---
id: "slice-research-claim-extractor"
kind: "internal_step"
owner_family: "slice_variant"
pipeline_id: "slice.research-to-durable-knowledge"
step_id: "slice-research-claim-extractor"
entry_gate: false
public_trigger: false
native_skill_discovery: false
body_path: "capabilities/instructions/pipelines/slice-variants/research-to-durable-knowledge/slice-research-claim-extractor.step.md"
source_manifest: "capabilities/pipelines/slice-variants/research-to-durable-knowledge.pipeline.json"
legacy_skill_ref: "slice-research-claim-extractor"
architecture_refs:
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#research-to-durable-kb-slice-variant"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19"
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#procedure-and-research-slice-variants"
---

# Slice Research Claim Extractor

## Overview
This skill extracts atomic research claims from already-collected evidence for the Research To Durable Knowledge Slice. Its core rule is claim-level traceability: every claim must carry source citation, claim type, confidence, freshness, scope, contradiction status, and durable-KB candidate boundary before later ledger, synthesis, or promotion work can use it.

## When to Use
Use this skill after evidence has been collected, ingested, and labeled for provenance and freshness in `slice.research-to-durable-knowledge`, and the next needed gate is `claims_extracted_with_sources`.

Use it when source material contains facts, observations, requirements, decisions, risks, comparisons, negative findings, or inferred relationships that must become source-backed rows in the intermediate `extracted-claims.md` carrier before ledger adjudication.

Do not use it to gather new evidence, browse live sources, dispatch research subagents, label provenance, resolve contradictions, synthesize findings, propose durable KB pages, promote claims, update indexes, capture procedures, debug behavior, or answer a simple current-state query.

## Source Contract
Grounding sources are `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#research-to-durable-kb-slice-variant`, `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#procedure-and-research-slice-variants`, `docs/architecture/master-plugin-target-architecture-part-6c-stateful-domain-pipelines.html#durable-kb-pipeline`, `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6`, and `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19`.

The owning manifest is `capabilities/pipelines/slice-variants/research-to-durable-knowledge.pipeline.json`, step `slice-research-claim-extractor`. The step is required, invokes `skill:slice-research-claim-extractor`, produces `extracted-claims.md`, gates on `claims_extracted_with_sources`, reaches `ready_for_next_step`, and fails by `stop_or_handoff`. The next ledger-builder step alone owns `claim-ledger.md`.

The registry classifies this as an Tect-owned internal `skill_body` with no external skill-body adaptation. It is extractive only: durable knowledge storage, promotion approval, contradiction reconciliation, synthesis, index/front-door updates, live research, and workspace mutation belong to later owners. The Part 6C durable KB pipeline owns canonical durable truth after promotion gates; this skill may only prepare source-backed claim rows for that later boundary.

## Operating Procedure
1. Check that the input packet contains the research questions or brief, `source-map.md`, `evidence-corpus/index.md`, `evidence-log.md`, `source-provenance.md`, `freshness-authority.md`, and any existing `negative-knowledge.md` or `contradictions-and-gaps.md` notes. If these inputs are absent, stop with the missing source list.
2. Set the extraction boundary from existing labeled evidence only. Accepted inputs are cited corpus entries, source-map records, evidence-log rows, provenance/freshness labels, and already-recorded negative or contradiction notes. Do not infer from memory, collect new sources, perform live research, execute researched code, shape a procedure, or treat a source as durable current truth merely because it was collected.
3. Walk each cited evidence unit and split it into atomic claims. One claim record should express one assertion, observation, relationship, requirement, risk, comparison, decision, negative finding, or explicit unknown. Preserve source wording when it changes meaning, and keep uncertainty visible.
4. Assign a claim type such as `observed_fact`, `source_assertion`, `derived_inference`, `requirement`, `decision`, `risk`, `constraint`, `comparison`, `negative_finding`, `contradiction_candidate`, or `open_question`.
5. Attach citation data for every claim: source id, path or URL when allowed, locator such as section/line/timestamp, evidence excerpt or concise paraphrase, source owner when known, provenance label, authority label, freshness label, and collection date or snapshot date when available.
6. Label confidence, freshness, and scope separately. Confidence reflects evidence support; freshness reflects currentness; scope names the product area, workspace, repository, domain, user segment, protocol, time window, or environment the claim applies to. Never upgrade stale or narrow evidence into broad current truth.
7. Mark contradiction and gap posture: `none`, `contradicted_by`, `needs_refresh`, `needs_authority`, `needs_primary_source`, `restricted_source`, `insufficient_evidence`, or `source_only`. Record negative, contradicted, stale, restricted, source-only, blocked, and rejected claims as explicit rows when the evidence supports them. Link to the contradicting or missing evidence pointer; do not reconcile, suppress, choose a winner, or smooth the conflict into final truth here.
8. Mark durable-KB candidate boundary: `candidate`, `candidate_with_restrictions`, `not_candidate`, `needs_domain_owner`, `needs_human_review`, or `defer`. Include proposed target owner only as a candidate route, not as promotion or canonical write approval.
9. Produce claim-ledger-ready rows and a short extraction note in `extracted-claims.md`. When the rows are source-backed, add the exact line `gate: claims_extracted_with_sources` and hand the carrier to `slice-research-claim-ledger-builder`. If every candidate claim lacks citation, authority, freshness, or scope, stop or hand off under `stop_or_handoff` instead of fabricating a ledger or writing `claim-ledger.md`.

## Outputs
The output is `extracted-claims.md`. Each record must include claim id, atomic statement, claim type, citation, evidence locator, provenance label, authority label, freshness label, confidence, scope, contradiction/gap status, durable-KB candidate boundary, proposed downstream owner when applicable, and next proof or review needed. A successful carrier includes exact line `gate: claims_extracted_with_sources`; it does not claim ledger adjudication.

The output may include rejected, negative, contradictory, stale, restricted, source-only, blocked, or deferred rows when they are useful durable research work. It must not promote claims, mutate durable KB storage, update indexes, synthesize final findings, resolve contradictions, dispatch subagents, collect new evidence, perform live checks, execute researched code, write canonical durable knowledge, or claim the research Slice is complete.

## Verification
Verify that every extracted claim maps to at least one cited evidence unit and that no row depends on uncited chat memory, generated projection alone, or an unlabeled raw source. Check that confidence, freshness, and scope are independent labels and that stale or narrow evidence stays constrained.

Verify the gate `claims_extracted_with_sources`: `extracted-claims.md` has source-backed rows and the exact gate line, or the step has an explicit stop/handoff reason naming missing evidence, missing provenance, missing freshness, contradiction, restricted material, or authority gaps. Verify that `claim-ledger.md` remains for the next owner, any durable-KB target is only a candidate route, and Part 6C canonical write gates remain unexecuted.

Validate the implementation with `node tools/validate-internal-skill-body-quality.mjs --skill slice-research-claim-extractor` and trigger coverage with `node tools/validate-internal-skill-trigger-fixtures.mjs --skill slice-research-claim-extractor`.

## Failure Modes
Stop or hand off when evidence has not been collected, provenance/freshness labels are missing, source locators are unavailable, source material is restricted beyond safe citation, or the claim cannot be scoped without inventing context.

Block progression when the caller asks this skill to browse, run external code, perform fresh research, resolve contradictions, synthesize conclusions, create a durable KB seed, approve promotion, update front doors or indexes, mutate workspace files, or treat a candidate route as accepted durable truth.

Route elsewhere when the task is source mapping, evidence planning, collection, ingestion, provenance labeling, claim-ledger status building, contradiction/gap classification, negative-knowledge preservation, synthesis, durable-object modeling, promotion gating, procedure capture, debug/root-cause work, or a direct query answer.
