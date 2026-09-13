---
id: "slice-research-kb-context-loader"
kind: "internal_step"
owner_family: "slice_variant"
pipeline_id: "slice.research-to-durable-knowledge"
step_id: "slice-research-kb-context-loader"
entry_gate: false
public_trigger: false
native_skill_discovery: false
body_path: "capabilities/instructions/pipelines/slice-variants/research-to-durable-knowledge/slice-research-kb-context-loader.step.md"
source_manifest: "capabilities/pipelines/slice-variants/research-to-durable-knowledge.pipeline.json"
legacy_skill_ref: "slice-research-kb-context-loader"
architecture_refs:
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#research-to-durable-kb-slice-variant"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19"
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#procedure-and-research-slice-variants"
---

# Slice Research KB Context Loader

## Overview
This skill loads the starting knowledge context for a Research To Durable Knowledge Slice. Its core rule is to make the parent spine state, prior durable knowledge, current indexes, prior research, and authority constraints explicit before downstream research questions, source maps, evidence plans, or promotion proposals are created.

## When to Use
Use this skill after the research entry gate has accepted the Slice variant and the next needed step is context loading for `slice.research-to-durable-knowledge`.

Use it when the worker needs to assemble current Program, Scope, Slice, Result, durable KB, index, prior research, and authority context into the Slice front door before research framing begins.

Do not use it for simple current-state questions that belong to query services, for debug/root-cause truth discovery, for procedure capture, for source-map design, for evidence collection, for claim extraction, for synthesis, for durable KB promotion, or for canonical knowledge writes.

## Source Contract
Grounding sources are `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#research-to-durable-kb-slice-variant`, `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#procedure-and-research-slice-variants`, `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6`, and `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19`.

The owning manifest is `capabilities/pipelines/slice-variants/research-to-durable-knowledge.pipeline.json`, step `slice-research-kb-context-loader`. The step is required, invokes `skill:slice-research-kb-context-loader`, produces `README.md`, gates on `context_loaded_or_declared_missing`, reaches `ready_for_next_step`, and fails by `stop_or_handoff`.

The registry record classifies this as an Tect-owned internal `skill_body` with no external skill body adaptation. Its authority is read/propose only; promotion requires explicit authority, and durable KB storage remains owned by the durable-domain pipeline.

## Operating Procedure
1. Confirm the active Slice variant is `slice.research-to-durable-knowledge`, that `slice-research-kb-entry-gate` has accepted the research target, and that this step is next in the runtime view.
2. Identify the parent chain to load: Program, Epoch when present, Scope, current Slice, any prior Result or promotion edge, and any target durable knowledge lane candidate.
3. Load the current front-door context for those objects: intent, scope boundary, accepted decisions, deferred work, prior evidence basis, proof requirements, current next action, and any stale or blocked status.
4. Load durable knowledge context without treating it as fresh authority by default: relevant KB indexes, object pages, runbooks, protocols, decision records, prior research packets, source maps, claim ledgers, negative knowledge, and promotion history.
5. Label every loaded item by source role and risk: canonical source, generated index, raw evidence, prior synthesis, memory-derived pointer, stale snapshot, restricted material, contradiction, open question, or missing source.
6. Separate route context from research work. Record what downstream steps should use for question framing, source-map building, evidence planning, task decomposition, evidence collection, provenance labeling, contradiction handling, synthesis, and promotion gating.
7. Produce the `README.md` context front door with the loaded context summary, source paths or pointers, freshness and authority notes, missing-context declarations, contradiction notes, restricted-source handling, and the explicit next step.
8. If required context cannot be loaded or safely referenced, do not approximate from memory. Declare the missing source, stale claim, contradiction, or authority gap and stop or hand off under `context_loaded_or_declared_missing`.

## Outputs
The output is the Slice `README.md` context front door for the research-to-durable-KB workflow. It must include the parent spine context, target durable knowledge lane candidate, relevant prior KB/index/research pointers, authority and freshness labels, evidence basis known so far, missing or restricted context, contradictions or negative knowledge already visible, and the next downstream step.

The output may propose reads, source classes, and handoff needs. It must not write durable KB pages, update indexes, promote claims, dispatch subagents, perform researched code execution, mutate workspace source, or claim research completion.

## Verification
Verify that the body contains exactly the seven Layer 6B sections, references the architecture HTML sources and `capabilities/pipelines/slice-variants/research-to-durable-knowledge.pipeline.json`, and keeps the step tied to `slice-research-kb-context-loader`.

Verify the trigger fixture has at least two positive cases for loading research Slice context and at least one negative case for downstream research, procedure, debug, query, promotion, or durable-write behavior.

Verify the produced `README.md` content satisfies `context_loaded_or_declared_missing`: context is either loaded with source/freshness/authority labels or the missing/stale/restricted context is explicitly declared with stop or handoff.

## Failure Modes
Stop or hand off when the parent Program, Scope, Slice, target durable lane, prior KB index, authority state, or freshness requirement is missing and cannot be safely reconstructed from approved sources.

Block progression when loaded sources contradict each other, include restricted material that cannot be summarized, depend on stale memory without a source path, or imply a durable write/promotion before evidence, provenance, freshness, and target-owner checks exist.

Route elsewhere when the request is only a fact lookup, a debug/root-cause investigation, a procedure capture candidate, evidence collection, synthesis, promotion, index update, or durable-domain write. This skill only loads and labels the starting context for the research Slice.
