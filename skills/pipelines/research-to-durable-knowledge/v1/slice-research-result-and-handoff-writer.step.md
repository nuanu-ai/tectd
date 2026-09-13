---
id: "slice-research-result-and-handoff-writer"
kind: "internal_step"
owner_family: "slice_variant"
pipeline_id: "slice.research-to-durable-knowledge"
step_id: "slice-research-result-and-handoff-writer"
entry_gate: false
public_trigger: false
native_skill_discovery: false
body_path: "capabilities/instructions/pipelines/slice-variants/research-to-durable-knowledge/slice-research-result-and-handoff-writer.step.md"
source_manifest: "capabilities/pipelines/slice-variants/research-to-durable-knowledge.pipeline.json"
legacy_skill_ref: "slice-research-result-and-handoff-writer"
architecture_refs:
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#research-to-durable-kb-slice-variant"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19"
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#procedure-and-research-slice-variants"
---

# Slice Research Result And Handoff Writer

## Overview

This skill closes a Research To Durable Knowledge Slice by producing the final `result.md` truth and handoff packet. Its core rule is: record the highest validated research truth, including every supported, gapped, blocked, and deferred claim, without promoting canonical durable knowledge or mutating indexes.

## When to Use

Use this when the selected `slice.research-to-durable-knowledge` lifecycle is at the manifest step `slice-research-result-and-handoff-writer`, the gate is `result_boundary_ready`, and the remaining job is closure: terminal truth, proof summary, promotion result, unresolved gaps, deferred work, durable-domain handoff, follow-up scopes, and next owner.

Do not use it for source collection, evidence ingestion, claim extraction, provenance labeling, contradiction classification, synthesis, durable object modeling, KB seed proposal writing, promotion-gate approval, canonical durable KB writing, index/front-door mutation, procedure capture, debug work, or simple query answering.

## Source Contract

Grounding comes from `capabilities/pipelines/slice-variants/research-to-durable-knowledge.pipeline.json`, where this required skill step produces `result.md`, gates on `result_boundary_ready`, fails by `stop_or_handoff`, and ends `ready_for_next_step`. The relevant architecture anchors are `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#research-to-durable-kb-slice-variant`, `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#procedure-and-research-slice-variants`, `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6`, `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19`, and `docs/architecture/master-plugin-target-architecture-capability-atom-shard-map.html#pipeline.slice.research_to_durable_knowledge.result.and.handoff.writer`.

The atom map defines this responsibility as terminal truth, gaps, blocked states, promotion result, follow-up scopes, and resume/handoff. External references are empty for this skill.

## Source Inputs

Require existing closure inputs before writing: `source-map.md`, `evidence-corpus/index.md`, `evidence-log.md`, `source-provenance.md`, `freshness-authority.md`, `claim-ledger.md`, `contradictions-and-gaps.md`, `negative-knowledge.md`, `synthesis.md`, `durable-kb-seed.md`, `promotion.md`, `deferred.md`, and the index/front-door check or deferral.

Treat optional artifacts such as `comparison-matrix.md`, `object-pages-draft/`, `runbook-draft.md`, `index-update-proposal.md`, and `handoff.md` as supporting evidence only. They can inform the result, but they do not replace claim-ledger proof, promotion-gate outcome, or accountable ownership.

## Operating Procedure

1. Confirm the gate is `result_boundary_ready`, the selected step is `slice-research-result-and-handoff-writer`, and all required source inputs exist or are explicitly named in `deferred.md`.
2. Reconstruct what was researched from `research-questions.md`, `source-map.md`, `evidence-log.md`, and `synthesis.md`. State the research scope, excluded questions, source classes, and evidence time horizon.
3. Separate final truth into supported, gapped, blocked, rejected, source-only, and deferred buckets. Tie every supported statement to claim-ledger rows and every gap or blocked state to contradiction, freshness, authority, or negative-knowledge evidence.
4. Record candidate durable KB seeds without promoting them. Point to `durable-kb-seed.md`, object/runbook/protocol candidates, restrictions, target durable-domain owner, promotion prerequisites, and the next proof needed.
5. Write `result.md` around the highest validated research truth. Include proof inventory, provenance/freshness/authority basis, promotion result, index/front-door status, forbidden claims, deferred work, follow-up scopes, resume context, and next owners.
6. Preserve every unresolved gap. Name stale claims, contradictions, negative knowledge, missing proof, owner decisions, source-only material, and follow-up scopes that should become Program, Scope, Slice, domain, or maintenance work.
7. Build the durable-domain handoff inside the result: target domain or artifact type, promotion candidate reference, restrictions, owner review, promotion prerequisites, maintenance/index needs, resume context, and next owner after gate decisions.
8. Stop instead of closing if a required input is absent, proof is missing, promotion gate is unresolved, index/front-door status is unknown, or no accountable next owner can be named.

## Outputs

The declared output is `result.md`.

It must contain result state, what was researched, highest validated research truth, supported/gapped/blocked/rejected/deferred claim table, evidence/proof inventory, promotion result, candidate durable KB seeds, promotion prerequisites, index/front-door status, durable-domain handoff, unresolved gaps, deferred work, forbidden claims, follow-up scopes, next owners, and resume/handoff packet.

It may point to existing proposals or optional handoff artifacts, but it must not create canonical durable KB content, accept promotion, repair indexes, mutate a front door, or rewrite upstream research artifacts.

## Terminal States

Use exactly one primary terminal state from the completion contract: `research_synthesized_not_promoted`, `durable_kb_seed_proposed`, `promoted_to_durable_kb`, `promoted_with_restrictions`, `blocked_by_missing_sources`, `blocked_by_freshness_or_authority`, `blocked_by_contradiction`, `rejected_insufficient_evidence`, `handoff_for_human_review`, or `escalated_to_durable_domain_pipeline`.

If more than one state seems applicable, choose the strongest state supported by proof and list secondary states as scoped follow-up or deferred work. The manifest step terminal state remains `ready_for_next_step` only after `result.md` names the primary state and the next owner.

## Verification

Verify that the skill only appears at the closure step after upstream research artifacts, promotion gate, deferred-work, and index/front-door decisions have been recorded. Check that `result.md` uses evidence-trace proof, names the terminal state, preserves contradictions and negative knowledge, identifies the durable-domain handoff owner, and lists every candidate durable KB seed with prerequisites.

Check that no claim says durable KB was mutated by the research Slice, no completion claim is stronger than available proof, no unsupported claim is recast as durable truth, and no unresolved gap disappears from the result. The result passes only when a later agent can resume from the handoff without rereading the whole corpus to discover blockers or owners.

## Forbidden Actions

- No source collection, evidence ingestion, claim extraction, provenance labeling, research synthesis, or durable object modeling inside this step.
- No KB seed proposal authorship, no promotion approval, no canonical durable KB write, no canonical durable KB mutation, no index/front-door mutation, and no repair of front-door discoverability from this skill.
- No procedure capture, no debug root-cause work, no simple query answering, no pipeline execution authorization, and no workspace mutation authorization.
- No hidden contradictions, no hidden negative knowledge, no hidden stale claims, and no completion without proof.

## Failure Modes

Block or hand off when closure inputs are missing, proof is stale or insufficient, source provenance is absent, freshness/authority labels are unresolved, contradictions remain undecided, negative knowledge was dropped, promotion gate or index/front-door status is absent, or the requested action is a canonical durable write. Route upstream work back to the owning research step; route canonical knowledge changes to the durable-domain owner; route stale result/index drift to maintenance; route ambiguous new scope to Program/Scope/Slice decomposition. End with zero unowned follow-up claims.

## Routing

Route missing research evidence to the relevant upstream research step. Route canonical knowledge changes to the stateful durable-domain pipeline owner. Route index/front-door drift to maintenance or index/front-door checker ownership. Route ambiguous new scope to Program/Scope/Slice decomposition. Route blocked authority, freshness, or contradiction states to human review with exact next proof and owner.
