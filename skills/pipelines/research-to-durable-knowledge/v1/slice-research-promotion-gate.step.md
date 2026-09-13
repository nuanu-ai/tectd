---
id: "slice-research-promotion-gate"
kind: "internal_step"
owner_family: "slice_variant"
pipeline_id: "slice.research-to-durable-knowledge"
step_id: "slice-research-promotion-gate"
entry_gate: false
public_trigger: false
native_skill_discovery: false
body_path: "capabilities/instructions/pipelines/slice-variants/research-to-durable-knowledge/slice-research-promotion-gate.step.md"
source_manifest: "capabilities/pipelines/slice-variants/research-to-durable-knowledge.pipeline.json"
legacy_skill_ref: "slice-research-promotion-gate"
architecture_refs:
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#research-to-durable-kb-slice-variant"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19"
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#procedure-and-research-slice-variants"
---

# Slice Research Promotion Gate

## Overview
This skill is the late-stage gate for `slice.research-to-durable-knowledge`. It decides whether a completed research packet may proceed toward durable-domain handoff, must stay restricted, or must stop, using evidence trace, provenance, freshness, authority, contradiction status, negative knowledge, synthesis, durable KB seed, promotion edge, and target owner fit.

## When to Use
Use this skill when a Research To Durable Knowledge Slice has already produced the required research packet and the next manifest step is `slice-research-promotion-gate`.

Use it to consume `promotion-edge.md` and decide whether final `promotion.md` records an approved, restricted, not-required, or blocked verdict. This step never writes `deferred.md`; the following index/front-door checker owns that final carrier.

Do not use it for earlier evidence gathering, evidence ingestion, atomic claim derivation, claim ledger construction, synthesis drafting, durable KB seed drafting, first-time promotion edge recording, discoverability checks, canonical durable knowledge writing, procedure/runbook workflows, debug/root-cause work, or direct factual lookup.

## Source Contract
Grounding sources are `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#research-to-durable-kb-slice-variant`, `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#procedure-and-research-slice-variants`, `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6`, and `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19`.

The owning manifest is `capabilities/pipelines/slice-variants/research-to-durable-knowledge.pipeline.json`. The `slice-research-promotion-gate` step is required, invokes `skill:slice-research-promotion-gate`, consumes `promotion-edge.md`, produces only `promotion.md`, gates on `promotion_gate_recorded`, reaches `ready_for_next_step`, and fails by `stop_or_handoff`.

Architecture and mapping anchors are `pipeline.slice.research-to-durable-knowledge`, `pipeline.slice.research_to_durable_knowledge.promotion.gate`, `capabilities/pipelines/slice-variants/research-to-durable-knowledge.pipeline.json#step_graph.steps.slice-research-promotion-gate`, and `capabilities/pipelines/slice-variants/research-to-durable-knowledge.pipeline.json#step_graph.steps.slice-research-promotion-gate.invokes.slice-research-promotion-gate`. External reference inputs are empty for this step. Durable-domain handoff is candidate-only here; storage-owner decisions and durable writes belong to later owners.

## Operating Procedure
1. Load the active Slice packet and confirm the gate is reached after the manifest predecessors: source map, evidence corpus, evidence log, source-provenance, freshness-authority, claim-ledger, contradictions-and-gaps, negative-knowledge, synthesis, durable-kb-seed, and `promotion-edge.md`.
2. For each candidate claim, verify the minimum promotion packet: evidence pointer, source role, source or snapshot date, provenance label, freshness status, authority or allowed-use label, contradiction disposition, negative knowledge relationship, synthesis reference, durable KB seed reference, target owner, and proposed durable-domain handoff lane.
3. Assign a per-claim disposition: accepted, accepted with restriction, source-only, stale, contradicted, unsafe, unsupported, unauthorized, rejected, or deferred. Preserve rejected or deferred facts as useful negative knowledge when they should inform future work.
4. Decide the packet verdict. Use `promoted_with_restrictions` only when every restriction is named and the next owner can honor it. Use `blocked_by_missing_sources`, `blocked_by_freshness_or_authority`, `blocked_by_contradiction`, or `rejected_insufficient_evidence` when the packet cannot safely proceed.
5. Check owner and authority fit. The target owner must be explicit; approval conditions must be named; durable-domain handoff must be a next-step proposal, not a silent durable write.
6. Write the gate record in `promotion.md` for every verdict. Include one exact verdict token: `promotion_approved`, `promotion_not_required`, or `promotion_blocked`; include accepted and excluded claims, evidence and proof links, provenance/freshness/authority labels, contradiction handling, negative knowledge carried forward, target owner, handoff lane, remaining conditions, and any exact blocker or next route.
7. Do not write `deferred.md`. A blocked verdict remains in `promotion.md` so `slice-research-index-front-door-checker` can inspect discoverability and become the sole owner of the final deferral record.
8. Add exact line `gate: promotion_gate_recorded` only after the verdict is explicit and internally traceable. Leave the step at `ready_for_next_step` only for a complete gate record; otherwise stop or hand off through `stop_or_handoff`.

## Outputs
`promotion.md` records every gate verdict, including blocked or not-required outcomes. It must include `gate: promotion_gate_recorded`, one exact verdict token, promoted claims, rejected or deferred claims, evidence and provenance links, freshness and authority labels, contradiction handling, negative knowledge summary, synthesis reference, durable KB seed and `promotion-edge.md` references, target owner, allowed durable-domain handoff, remaining conditions, and the next route. `deferred.md` is forbidden from this step.

## Verification
Verify that the gate verdict is traceable to the required research artifacts and that every promoted or restricted claim has evidence corpus coverage, source map linkage, provenance, freshness, authority, contradiction disposition, negative knowledge treatment, synthesis support, durable KB seed linkage, target owner, and durable-domain handoff condition.

Verify the body keeps exactly the seven Layer 6B sections, references the required architecture HTML sources, references `capabilities/pipelines/slice-variants/research-to-durable-knowledge.pipeline.json`, and keeps the step tied to `slice-research-promotion-gate` and `promotion_gate_recorded`.

Verify trigger fixtures include at least two positive cases for promotion readiness decisions and at least one negative case for earlier research work, edge creation, discoverability checks, durable writing, procedure/runbook work, debug work, or direct factual lookup.

## Failure Modes
Stop or hand off when source map, evidence corpus, claim ledger, provenance, freshness, authority, contradictions-and-gaps, negative knowledge, synthesis, durable KB seed proposal, promotion edge, target owner, or approval state is missing or internally inconsistent.

Block promotion when a claim is source-only, stale, contradicted, unsafe, restricted without handling, unsupported by evidence, outside the target durable lane, or dependent on live/current truth that was not refreshed. Preserve those findings as `promotion_blocked` details in `promotion.md` and negative knowledge rather than hiding them; let the next checker own `deferred.md`.

Route elsewhere when the actual request belongs to earlier research steps, first-time edge recording, discoverability maintenance, durable-domain writing, procedure/runbook workflow, debug/root-cause work, direct factual lookup, Program/Scope/Slice decomposition, or human authority resolution.

Treat unresolved authorization for promotion, target-owner handoff, restricted evidence, or durable-domain routing as a blocker that must be named before the Slice advances.
