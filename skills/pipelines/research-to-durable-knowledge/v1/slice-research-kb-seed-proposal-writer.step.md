---
id: "slice-research-kb-seed-proposal-writer"
kind: "internal_step"
owner_family: "slice_variant"
pipeline_id: "slice.research-to-durable-knowledge"
step_id: "slice-research-kb-seed-proposal-writer"
entry_gate: false
public_trigger: false
native_skill_discovery: false
body_path: "capabilities/instructions/pipelines/slice-variants/research-to-durable-knowledge/slice-research-kb-seed-proposal-writer.step.md"
source_manifest: "capabilities/pipelines/slice-variants/research-to-durable-knowledge.pipeline.json"
legacy_skill_ref: "slice-research-kb-seed-proposal-writer"
architecture_refs:
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#research-to-durable-kb-slice-variant"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19"
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#procedure-and-research-slice-variants"
---

# Research KB Seed Proposal Writer

## Overview
This skill drafts the proposal-only `durable-kb-seed.md` for a Research To Durable Knowledge Slice. Its core rule is that research becomes a cited seed candidate, not canonical durable truth, until promotion readiness and the target owner accept it.

## When to Use
Use this after evidence collection, provenance and freshness labeling, claim-ledger completion, contradiction/gap classification, negative knowledge capture, synthesis, and durable object modeling are available. Select it when the next task is to turn already-modeled candidate objects into a reviewable KB seed proposal with claim-backed sections, source citations, authority/freshness labels, target lane, and promotion prerequisites.

Do not use it to gather sources, extract claims, model durable objects from scratch, approve promotion, mutate durable KB, update indexes/front doors, or answer a direct current-state query. If the object model or claim proof is missing, route back to the owning upstream research step.

## Source Contract
Ground this skill in `capabilities/pipelines/slice-variants/research-to-durable-knowledge.pipeline.json#step_graph.steps.slice-research-kb-seed-proposal-writer`, `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#research-to-durable-kb-slice-variant`, `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#procedure-and-research-slice-variants`, `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6`, `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19`, and atom rows `pipeline.slice.research_to_durable_knowledge.kb.seed.proposal.writer` plus `pipeline.slice.research-to-durable-knowledge`.

The manifest step is required, invokes `slice-research-kb-seed-proposal-writer`, produces `durable-kb-seed.md`, gates on `kb_seed_proposal_written`, reaches `ready_for_next_step`, and stops or hands off on failure. External reference inputs for this record are empty.

## Operating Procedure
1. Verify the packet contains the parent Slice/Result identity, target durable lane, source-provenance.md, freshness-authority.md, claim-ledger.md, contradictions-and-gaps.md, negative-knowledge.md, synthesis.md, and the proposal-only `durable-object-model.md`. If any required input is absent, stop with the missing artifact named.
2. Open the seed with proposal status, target lane, intended durable owner, source date range, freshness posture, authority posture, and explicit statement that no canonical durable write has occurred.
3. For each candidate object, create a compact section with title, object type, owner domain, proposed canonical location or handoff lane, included claim IDs, excluded claim IDs, and the reason it belongs in this seed instead of another domain.
4. Draft proposed wording only from ledger entries. Every factual sentence that could become durable truth must cite claim IDs and evidence/source links; unsupported implications become questions, gaps, hypotheses, or deferred items.
5. For each proposed paragraph, include evidence and limits: source claim refs, source links, confidence, allowed use, freshness date, authority class, contradiction status, and known proof gap.
6. Carry over provenance, freshness, and authority labels beside the relevant claims. Mark live/current, snapshot, historical, advisory, stale-risk, restricted, or low-authority material explicitly rather than blending labels into prose.
7. Record target placement and lifecycle decision per candidate: create new object, update existing object, merge with existing object, retire stale object, route to another domain, no-promote, or defer. Tie the decision to existing-match evidence when available.
8. Preserve contradiction/gap and negative-knowledge visibility. Add sections for contradicted claims, missing proof, rejected findings, unsafe or restricted material, stale claims, source-only facts, and non-publishable findings, with the exact reason each cannot be promoted now.
9. List promotion prerequisites per candidate: owner review, freshness refresh, authority approval, contradiction resolution, redaction, target-domain route, index/front-door proposal, maintenance trigger, or human decision. Do not mark a prerequisite complete unless the input packet proves it.
10. Finish with a handoff block for `slice-research-promotion-edge-writer` and `slice-research-promotion-gate`: ready candidates, blocked candidates, deferred work, proof gaps, authority needs, target lane, placement decision, and no-write boundary.

## Outputs
The output is `durable-kb-seed.md` as a Slice-local proposal artifact derived from `durable-object-model.md`. It must contain proposal status, target durable lane, object model summary, proposed wording, claim-backed sections with citations, source claim refs, evidence/limits, provenance/freshness/authority labels, target placement, update/merge/retire/no-promote decisions, contradiction and gap inventory, negative knowledge, proof gaps, authority needs, deferred or rejected material, promotion prerequisites, and a handoff block for promotion review. A complete proposal includes exact line `gate: kb_seed_proposal_written`.

Artifact placement stays inside the active Slice until a later owner accepts promotion and performs any canonical durable write.

The only valid success posture is `ready_for_next_step` after `kb_seed_proposal_written` is satisfied. The seed may recommend promotion, no-promotion, human review, route to a durable-domain pipeline, update, merge, retire, or deferral, but it must not mutate canonical KB, accept promotion, update indexes, update front doors, or hide proof limits.

## Verification
Check that each proposed durable claim traces to claim-ledger IDs and evidence/source links, and that each source carries a freshness and authority label. Confirm proposed wording, evidence/limits, target placement, update/merge/retire/no-promote decisions, proof gaps, authority needs, contradictions, gaps, source-only facts, stale material, restricted material, and negative findings remain visible in the seed instead of being smoothed into final prose.

Verify the body references the owning manifest and architecture paths above, keeps exactly the seven Layer 6B sections, and contains no wrapper-only headings. Validate scenarios where the synthesis is strong but lacks claim proof, where claims are contradicted or stale, where the target lane belongs to another domain, and where a user asks for immediate canonical KB mutation. The correct result is a proposal-only `durable-kb-seed.md` or a blocked handoff.

## Failure Modes
Stop or hand off when the packet lacks a durable object model, claim IDs, citations, source-provenance, freshness/authority labels, contradiction/gap review, negative knowledge, target lane, owner domain, or promotion prerequisites. Block the seed when evidence is stale, authority is absent, sensitive material is unresolved, contradictions affect core claims, or the target durable owner is unclear.

Route elsewhere when the task is source collection, evidence ingestion, claim extraction, durable object modeling, promotion-edge writing, promotion-gate evaluation, index/front-door checking, procedure capture, direct query answering, or canonical durable-domain writing. Never convert raw evidence, source-only notes, or useful synthesis into accepted durable truth without proof, owner acceptance, and the downstream promotion gate.

Forbidden actions: do not write canonical KB pages, mutate durable object storage, update durable indexes, update front doors, accept promotion, execute researched code, or collapse the promotion gate into this proposal writer.

Record zero-proof shortcuts as blockers inside `durable-kb-seed.md` or a handoff packet: if the seed would require uncited prose, unlabeled freshness, unlabeled authority, negative knowledge concealed from review, contradictions concealed from review, missing placement evidence, missing lifecycle decision, or an implicit durable write, do not write the later step's `deferred.md` or complete the gate.
