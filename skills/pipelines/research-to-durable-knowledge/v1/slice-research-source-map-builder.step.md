---
id: "slice-research-source-map-builder"
kind: "internal_step"
owner_family: "slice_variant"
pipeline_id: "slice.research-to-durable-knowledge"
step_id: "slice-research-source-map-builder"
entry_gate: false
public_trigger: false
native_skill_discovery: false
body_path: "capabilities/instructions/pipelines/slice-variants/research-to-durable-knowledge/slice-research-source-map-builder.step.md"
source_manifest: "capabilities/pipelines/slice-variants/research-to-durable-knowledge.pipeline.json"
legacy_skill_ref: "slice-research-source-map-builder"
architecture_refs:
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#research-to-durable-kb-slice-variant"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19"
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#procedure-and-research-slice-variants"
---

# Slice Research Source Map Builder

## Overview

This skill creates `source-map.md` for a Research To Durable Knowledge Slice after `research-questions.md` and any declared research tasks exist, and before evidence planning or collection begins. Its core rule is to map candidate source terrain, source authority, provenance expectations, freshness requirements, access, safety, contradictions, negative-source probes, and gaps without collecting evidence or turning any source into a claim.

## When to Use

Use this when the active research Slice needs `source_map_built` before `evidence-plan.md`, especially when explicit research questions or research tasks need source families across local docs, code, git history, sessions, logs, transcripts, external repos, web pages, runtime snapshots, datasets, standards, vendor docs, or human-provided material.

Do not use it to frame the research questions, plan collection order, collect or ingest evidence, perform post-collection provenance labeling, extract claims, finalize claims, reconcile contradictions, write synthesis, propose a KB seed, approve promotion, write durable knowledge, capture procedures, debug root cause, or answer a simple query directly. The shorthand boundary is: no evidence collection, no provenance labeling, no claim extraction, no claim finalization, no synthesis, and no promotion.

## Source Contract

Ground this skill in `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#research-to-durable-kb-slice-variant`, `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#procedure-and-research-slice-variants`, `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6`, `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19`, and `docs/architecture/master-plugin-target-architecture-capability-atom-shard-map.html#pipeline.slice.research_to_durable_knowledge.source.map.builder`.

The owning manifest is `capabilities/pipelines/slice-variants/research-to-durable-knowledge.pipeline.json`, step `slice-research-source-map-builder`, producing `source-map.md`, gate `source_map_built`, step terminal state `ready_for_next_step`, and failure route `stop_or_handoff`. Related variant-level blocked states include `blocked_by_missing_sources`, `blocked_by_freshness_or_authority`, and `blocked_by_contradiction`, but this step records those as source-map gaps or handoff reasons instead of resolving them. External reference sources are empty for this step.

## Operating Procedure

1. Load the Slice context, `research-brief.md`, `research-questions.md`, declared research tasks, target durable lane candidate, freshness requirement, authority state, and any seed source scope. If research questions, research tasks, or target decisions are missing, stop rather than inventing them.
2. Derive source families from each question, task, and decision need: local documentation, source code, tests, git history, issue or PR history, sessions, raw transcripts, logs, runtime snapshots, config or deployment evidence, external repos, official docs, standards, vendor docs, web pages, datasets, media, and human-provided material.
3. For each family, assign a role: primary authority, corroborating evidence, historical context, negative-search target, contradiction probe, freshness monitor, implementation evidence, operational evidence, or background only.
4. Record locators without collecting contents: repo path, doc path, command or query to use later, URL, branch or tag, log source, transcript path, session identifier, runtime endpoint, owner, access requirement, and expected retrieval method.
5. Classify authority and freshness needs before collection: canonical, owner-maintained, primary source, official but possibly stale, generated projection, historical snapshot, user-supplied, third-party, untrusted, private, restricted, or unsafe. Mark which future claims will need live refresh, owner review, timestamped snapshots, source-provenance labeling, or multiple source classes.
6. Define allowed and forbidden source classes for this Slice. Forbid executing researched external code, installing packages, mutating repos, scraping private material without authority, treating raw memory or sessions as current durable truth, or relying on uncited summaries when source custody is required.
7. Add negative and contradictory source handling before collection: name where disconfirming evidence is likely to exist, which source class could contradict the expected answer, which stale or rejected material should be checked, and which contradiction probes must be handed to evidence planning.
8. Convert the source map into evidence-plan inputs: proposed source family, locator, role, authority/freshness check, safety constraint, likely collector, expected artifact target, contradiction or negative-knowledge probe, and unresolved gap. Do not perform ordering, subagent assignment, evidence capture, provenance labeling, corpus ingestion, claim extraction, claim finalization, synthesis, or promotion; those belong to later steps.
9. Close with gap decisions: missing sources, missing authority, unavailable access, unknown freshness, unsafe source class, contradiction probes required, negative-source probes required, and whether to continue to evidence planning as `ready_for_next_step`, stop for user/source access, or hand off via `stop_or_handoff`.

## Outputs

Write `source-map.md` as a Slice-local planning artifact. It should contain a question/task-to-source matrix, source family taxonomy, source locators, source roles, source authority classes, provenance and freshness requirements, allowed and forbidden source classes, negative-source and contradiction probes, privacy or safety notes, expected evidence-plan inputs, open gaps, and the transition decision for `source_map_built`.

The output is not `evidence-plan.md`, `evidence-corpus/index.md`, `source-provenance.md`, `freshness-authority.md`, `claim-ledger.md`, `synthesis.md`, `durable-kb-seed.md`, `promotion.md`, a procedure proposal, a debug report, or a direct answer. It must not claim `research_synthesized_not_promoted`, `durable_kb_seed_proposed`, `promoted_to_durable_kb`, `promoted_with_restrictions`, or `rejected_insufficient_evidence`; those terminal states belong to later research steps.

## Verification

Verify that every research question, research task, or declared decision need has at least one mapped source family or an explicit gap. Confirm that each mapped source has a locator, intended role, authority class, provenance expectation, freshness requirement, access/safety boundary, negative or contradiction role when relevant, and downstream evidence-plan input.

Check that no evidence was collected, ingested, quoted as proof, provenance-labeled as collected evidence, promoted into a claim, finalized as a claim, synthesized, or written to durable knowledge by this step. The gate may move to `ready_for_next_step` only when `source-map.md` gives the evidence planner enough source targets, constraints, contradiction probes, negative-source probes, and gaps to design collection work.

## Failure Modes

Use `stop_or_handoff` when research questions or research tasks are absent, the durable target lane is unknown, source authority cannot be classified, required sources are inaccessible, freshness requirements cannot be met, contradiction or negative-source probes cannot be named, safety restrictions forbid the likely source class, or the user is really asking for simple query answering, debugging, procedure capture, synthesis, promotion, claim finalization, or durable writes.

Block rather than soften the requirement if the only available material is a stale summary, untrusted third-party copy, private memory without permission, uncited transcript excerpt, executable external project, or generated projection that cannot support the target durable claim.

Record zero-proof shortcuts as blockers: a source map is not complete if it omits access limits, relies on vague "search the web" instructions, lacks source roles, hides forbidden source classes, or sends the evidence planner into collection work without named locators and authority/freshness checks.
