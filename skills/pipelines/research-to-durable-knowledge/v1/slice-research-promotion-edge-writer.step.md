---
id: "slice-research-promotion-edge-writer"
kind: "internal_step"
owner_family: "slice_variant"
pipeline_id: "slice.research-to-durable-knowledge"
step_id: "slice-research-promotion-edge-writer"
entry_gate: false
public_trigger: false
native_skill_discovery: false
body_path: "capabilities/instructions/pipelines/slice-variants/research-to-durable-knowledge/slice-research-promotion-edge-writer.step.md"
source_manifest: "capabilities/pipelines/slice-variants/research-to-durable-knowledge.pipeline.json"
legacy_skill_ref: "slice-research-promotion-edge-writer"
architecture_refs:
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#research-to-durable-kb-slice-variant"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19"
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#procedure-and-research-slice-variants"
---

# Research Promotion Edge Writer

## Overview

This skill writes the promotion edge for a Research To Durable Knowledge Slice. Its job is to make promoted or restricted durable knowledge auditable by linking each target artifact back to claim IDs, source evidence, provenance, freshness, authority, restrictions, and residual risk. The output is the audit trail that lets future agents verify why a finding was promoted, restricted, rejected, blocked, or deferred.

It is a proposal and trace-writing skill. It does not approve promotion, create the KB seed, mutate canonical durable knowledge, update indexes, collect sources, extract claims, capture procedures, debug failures, or answer simple current-state questions.

## When to Use

Use this after the research slice has a completed claim ledger, synthesis, and durable KB seed proposal, but before the promotion gate decides the verdict. Select it when the next missing artifact is `promotion-edge.md` and the required gate is `promotion_edge_written`.

Do not use it for source collection, evidence ingestion, provenance labeling, claim extraction, contradiction classification, durable object modeling, `durable-kb-seed.md`, promotion approval, canonical KB page writing, runbook/protocol/domain writes, index/front-door updates, procedure capture, debug/root-cause work, or direct query answers.

## Source Contract

Ground this behavior in `capabilities/pipelines/slice-variants/research-to-durable-knowledge.pipeline.json`, step `slice-research-promotion-edge-writer`, which produces `promotion-edge.md`, gates on `promotion_edge_written`, fails by `stop_or_handoff`, and reaches `ready_for_next_step`. The following gate step alone owns final `promotion.md`.

Architecture anchors:

- `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#research-to-durable-kb-slice-variant`
- `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#procedure-and-research-slice-variants`
- `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6`
- `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19`
- `docs/architecture/master-plugin-target-architecture-capability-atom-shard-map.html#pipeline.slice.research-to-durable-knowledge`
- `docs/architecture/master-plugin-target-architecture-capability-atom-shard-map.html#pipeline.slice.research_to_durable_knowledge.promotion.edge.writer`

The atom map defines the responsibility as recording evidence-to-promoted-artifact edges for auditing durable claims. External references are empty for this skill.

## Operating Procedure

1. Confirm the input packet is ready for gate review but has no invented promotion decision. Require the parent Slice id, target durable object or domain, candidate claim IDs, evidence links, provenance labels, freshness labels, authority labels, contradiction/gap status, negative-knowledge disposition, and any restrictions or human-review notes.
2. Group each edge by target artifact: durable KB object page, runbook, protocol, decision record, issue or follow-up scope, or index/front-door proposal. Preserve rejected, no-promote, restricted, stale, source-only, and deferred items instead of dropping them.
3. For every promoted or restricted edge, record: target artifact, source claim IDs, supporting evidence citations, source path or URL, source owner when known, snapshot/live/current status, freshness date or unknown marker, authority level, allowed use, restriction, residual risk, and next proof needed.
4. Check the edge for traceability. Every durable claim must point back to at least one evidence item and one claim-ledger row. If either is missing, mark the edge blocked or deferred and route back to the owning upstream research step instead of inventing support.
5. Write `promotion-edge.md` as a Slice-local candidate trace artifact. Separate candidate, restricted, no-promote, blocked, and deferred edge evidence without deciding the promotion verdict; the next gate consumes this packet.
6. Preserve boundaries explicitly. The promotion edge records what can be audited; it does not make the promotion decision, does not write canonical durable KB storage, and does not update discovery indexes or front doors.
7. End with `ready_for_next_step` only when `promotion-edge.md` contains every candidate target and unresolved item plus exact line `gate: promotion_edge_written`. Otherwise stop or hand off with the missing evidence, authority, freshness, or contradiction reason.

## Outputs

Primary output is `promotion-edge.md`. It should contain a promotion packet summary, per-target evidence-to-artifact edge table, candidate edges, restricted edges, rejected/no-promote findings, blocked or deferred findings, residual risks, required follow-up proof, and handoff notes for `slice-research-promotion-gate`. It includes exact line `gate: promotion_edge_written` only when complete.

The successful terminal state is `ready_for_next_step` with the `promotion_edge_written` gate satisfied. Failure output is a stop or handoff note that names the missing claim rows, evidence citations, authority decision, freshness proof, target object, or contradiction resolution.

## Verification

Verify the trigger by checking that the task asks for trace edges before a research promotion gate decision, not for upstream research or downstream durable writes. Confirm the final body references `capabilities/pipelines/slice-variants/research-to-durable-knowledge.pipeline.json`, at least one `docs/architecture/*.html` source, `promotion-edge.md`, `promotion_edge_written`, and `ready_for_next_step`.

Verify the artifact by sampling each proposed edge: claim ID exists, evidence citation exists, provenance and freshness are labeled, authority and restrictions are stated, target artifact is named, and rejected or deferred material remains visible. Run the body-quality and trigger-fixture validators for this skill before claiming it is ready.

## Failure Modes

Block or hand off when the promotion gate decision is missing, the durable KB seed proposal is absent, target artifacts are unresolved, claim IDs do not exist, evidence citations are missing, provenance or freshness labels are unknown and material, authority is insufficient, contradictions are unresolved, or the requested action is a canonical durable-domain write.

Route elsewhere when the user needs evidence collection, claim-row derivation, KB seed drafting, promotion approval, index/front-door checking, reusable operation capture, failure-cause investigation, or a simple evidence-backed answer. If the task would silently mutate durable knowledge, execute researched code, hide stale findings, or upgrade unsupported claims, stop with a boundary failure.

zero-proof shortcuts are blockers: no claim may receive an edge without cited evidence, no target may be implied without a named durable object, and no restriction may be omitted because it is inconvenient for the final result. Leave those rows deferred or blocked.
