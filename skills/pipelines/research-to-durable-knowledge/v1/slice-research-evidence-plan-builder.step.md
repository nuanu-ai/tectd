---
id: "slice-research-evidence-plan-builder"
kind: "internal_step"
owner_family: "slice_variant"
pipeline_id: "slice.research-to-durable-knowledge"
step_id: "slice-research-evidence-plan-builder"
entry_gate: false
public_trigger: false
native_skill_discovery: false
body_path: "capabilities/instructions/pipelines/slice-variants/research-to-durable-knowledge/slice-research-evidence-plan-builder.step.md"
source_manifest: "capabilities/pipelines/slice-variants/research-to-durable-knowledge.pipeline.json"
legacy_skill_ref: "slice-research-evidence-plan-builder"
architecture_refs:
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#research-to-durable-kb-slice-variant"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19"
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#procedure-and-research-slice-variants"
---

# Research Evidence Plan Builder

## Overview
This skill turns framed research questions and a source map into a bounded `evidence-plan.md` for the research-to-durable-knowledge Slice. Its core rule: plan evidence collection, proof standards, source safety, delegation shape, and negative-finding capture before anyone collects, ingests, synthesizes, or promotes research.

Classification: `skill_body`. External references: none.

## When to Use
Use this after `research-questions.md` and `source-map.md` exist or their equivalent fields are present in the active Slice packet. The request should need durable reuse, claim-level evidence, source freshness decisions, task decomposition, or subagent-ready research passes.

Use it when the next decision is how to collect evidence across local docs, code, git history, memories, sessions, logs, transcripts, external repos, web sources, package registries, runtime snapshots, or official/current authority surfaces. It is especially important when some sources require freshness checks, read-only live probes, privacy filtering, or malware/no-execution boundaries.

Do not use it for simple fact lookup, source-map creation, research question framing, evidence collection, evidence ingestion, claim extraction, synthesis, durable KB seed writing, promotion approval, index updates, source repo mutation, deployment, installer execution, or live-system action.

## Source Contract
Ground this skill in `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#research-to-durable-kb-slice-variant`, `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#procedure-and-research-slice-variants`, `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6`, and `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19`.

The owning manifest is `capabilities/pipelines/slice-variants/research-to-durable-knowledge.pipeline.json`, step `slice-research-evidence-plan-builder`. The manifest marks this required skill as producing only `evidence-plan.md`, gating on `evidence_plan_built`, failing by `stop_or_handoff`, and continuing to later task decomposition, subagent assignment, evidence collection, ingestion, provenance labeling, claim work, synthesis, and promotion gates.

Atom and manifest anchors are `pipeline.slice.research-to-durable-knowledge`, `pipeline.slice.research_to_durable_knowledge.evidence.plan.builder`, and `capabilities/pipelines/slice-variants/research-to-durable-knowledge.pipeline.json#step_graph.steps.slice-research-evidence-plan-builder`. This skill may read the active Slice packet, research brief, questions, source map, authority state, freshness requirement, context packet, and prior KB context. It must not collect raw evidence, mutate durable knowledge, run external code, invoke installers, execute live-system commands, or promote claims.

## Operating Procedure
1. Verify entry inputs. Require the research brief or Slice contract, framed research questions, source map seed, authority state, freshness requirement, target durable lane candidate, and context budget. If any are missing, stop or route back to the prior research step.
2. Convert each research question into evidence needs. For every question, state the decision it supports, acceptable confidence level, required source classes, explicit source exclusions, and the minimum evidence needed to avoid an unsupported claim.
3. Classify source classes. Separate local canonical sources, generated projections, memory/session history, transcripts/media, git history, source code, issue/CI/deploy traces, external docs, external repos, web pages, package registries, runtime snapshots, and live service/API probes. Mark each as authoritative, advisory, historical, derived, stale-risk, restricted, unsafe, or blocked.
4. Declare authority and freshness needs before planning collection. For each source class, specify whether snapshot evidence is enough, whether current live verification is required, what timestamp or version must be captured, and what approval is needed for private, authenticated, paid, sensitive, or operational probes.
5. Define allowed and disallowed probes. Allowed probes are read-only and bounded: local file reads, repo/git inspection, citation-preserving web/doc reads, official metadata lookups, read-only API checks, and screenshot/transcript inspection when authority permits. Disallow installers, cloned code execution, package scripts, unknown binaries, write-capable APIs, credential exposure, source repo mutation, durable KB writes, deployment, scraping outside scope, and live commands that can change state.
6. Choose collection order. Prefer highest-authority and lowest-risk sources first, then freshness probes, then lower-authority corroboration, then contradiction checks. Put high-context or high-risk sources behind explicit gates. Keep collection steps small enough that a later collector can prove what was read and why.
7. Add evidence acceptance criteria. For each planned item, include target question, source class, exact path/query/URL or discovery route, collection method, proof standard, freshness label, authority label, expected artifact location, acceptance criteria, rejection criteria, privacy/safety limits, and required citation or excerpt policy.
8. Plan negative-finding capture. Require later collectors to record searched locations, search terms, missing-source reasons, unavailable/private sources, stale-only evidence, contradicted evidence, unsafe sources, and blocked probes. A no-evidence result is usable only when the bounded search scope is visible.
9. Shape subagent-ready decomposition when useful. Split work by independent question, source cluster, authority tier, or freshness probe. Each proposed subagent task must have a source limit, no-mutation rule, expected report shape, evidence-return contract, context budget, fan-in key, and stop condition. Do not dispatch subagents from this step.
10. Write `evidence-plan.md`. Keep it as a plan, not a corpus. End with the gate verdict, terminal state, next route, and remaining blockers. Use `evidence_plan_built` and `ready_for_next_step` only when the plan enables task decomposition or collection without hidden authority, freshness, safety, or context-budget gaps.

## Outputs
The required output is `evidence-plan.md`. It must contain the research questions covered, collection objectives, source-class matrix, authority and freshness requirements, allowed probes, disallowed probes, collection order, evidence acceptance criteria, negative-finding requirements, context-budget allocation, subagent decomposition recommendations when appropriate, stop conditions, and next manifest owner.

The artifact shape is:
- `inputs`: Slice id, research brief, research questions, source-map reference, authority state, freshness requirement, target durable lane, and context budget.
- `question_plan`: one row per question with decision need, source classes, confidence bar, exclusions, and proof requirement.
- `collection_plan`: ordered read-only probes with path/query/URL or discovery route, acceptance criteria, rejection criteria, citation policy, privacy/safety limit, and expected downstream evidence location.
- `negative_findings`: required search logs, missing-source reasons, blocked probes, stale-only evidence, contradictions to preserve, and confidence limits.
- `decomposition`: optional subagent-ready task suggestions with source limits, no-mutation rule, report shape, evidence-return contract, fan-in key, and stop condition.
- `gate`: one of `evidence_plan_built` or `stop_or_handoff`, terminal state, next route, blocked reason, resume condition, and owner.

The output may name downstream artifacts such as `task-decomposition.md`, `subagent-assignments.md`, `evidence-corpus/index.md`, `evidence-log.md`, `source-provenance.md`, `freshness-authority.md`, `claim-ledger.md`, `negative-knowledge.md`, and `synthesis.md`, but it must not create or fill those artifacts. It must not collect raw sources, ingest evidence, extract claims, synthesize findings, propose durable KB seeds, approve promotion, update indexes, or modify durable-domain content.

Success means the plan supports safe later work under gate `evidence_plan_built` and terminal state `ready_for_next_step`. The normal next route is `slice-research-task-decomposer`; if decomposition is already represented in the active packet, route to `slice-research-subagent-assignment-manager` or `slice-research-evidence-collector` only with the same plan constraints. Blocked outputs use `stop_or_handoff` and route back to `slice-research-question-framer`, `slice-research-source-map-builder`, human review, maintenance/query, debug/root-cause, procedure capture, operational execution, or durable-domain pipeline as the boundary requires. Do not use final research terminal states such as `research_synthesized_not_promoted`, `durable_kb_seed_proposed`, `promoted_to_durable_kb`, `promoted_with_restrictions`, `blocked_by_contradiction`, `rejected_insufficient_evidence`, or `handoff_for_human_review`; those belong to later research Slice owners.

## Verification
Run `node tools/validate-internal-skill-body-quality.mjs --skill slice-research-evidence-plan-builder` and `node tools/validate-internal-skill-trigger-fixtures.mjs --skill slice-research-evidence-plan-builder`. Parse both owned fixture JSON files, check that this skill has exactly the seven Layer 6B H2 sections, and run scoped whitespace and diff checks on only this skill and its two fixtures.

For content verification, inspect `evidence-plan.md`: it must map each research question to source classes, authority/freshness needs, read-only collection order, allowed and disallowed probes, evidence acceptance criteria, negative-finding capture, context-budget limits, and subagent-ready decomposition where useful. It must also explicitly say that this step performs no collection, ingestion, synthesis, durable write, promotion, deployment, installer execution, or live mutating action.

Trigger verification should prove selection only after research questions and source map exist, and rejection when the task is simple lookup, question framing, source mapping, active collection, durable KB mutation, promotion, or operational execution.

## Failure Modes
Return `stop_or_handoff` when research questions are missing, the source map is absent or too vague, authority is unknown, freshness requirements cannot be met safely, source classes are private or restricted without approval, required live probes would mutate state, untrusted external code would need execution, the context budget cannot support credible collection, or explicit authorization is missing for restricted access.

Block instead of planning when the request is really a debug/root-cause Slice, procedure capture, product research domain pipeline, direct durable-domain write, current-state query, or operational execution. Route back to question framing or source mapping when the plan would otherwise invent scope.

Hand off when evidence acceptance criteria depend on a human decision, paid/private access, legal or security review, sensitive transcripts, credentials, production access, or a conflict between source authority and user intent. Preserve negative-finding requirements and blocked probe details so later research, maintenance, or durable-domain owners can continue without hiding gaps. If the user asks for immediate truth instead of a plan, route to query/current-state handling; if the user asks for final findings, route to evidence collection, ingestion, provenance/freshness labeling, claim extraction, synthesis, or promotion gate as appropriate.

Record the blocked reason, next owner, missing input, unsafe probe, and resume condition in the plan or handoff packet so downstream research can continue from the same boundary instead of repeating unsafe discovery. Keep those fields explicit rather than adding new terminology outside the manifest, source map, or handoff owner contract.
