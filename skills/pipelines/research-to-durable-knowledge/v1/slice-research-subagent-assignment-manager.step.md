---
id: "slice-research-subagent-assignment-manager"
kind: "internal_step"
owner_family: "slice_variant"
pipeline_id: "slice.research-to-durable-knowledge"
step_id: "slice-research-subagent-assignment-manager"
entry_gate: false
public_trigger: false
native_skill_discovery: false
body_path: "capabilities/instructions/pipelines/slice-variants/research-to-durable-knowledge/slice-research-subagent-assignment-manager.step.md"
source_manifest: "capabilities/pipelines/slice-variants/research-to-durable-knowledge.pipeline.json"
legacy_skill_ref: "slice-research-subagent-assignment-manager"
architecture_refs:
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#procedure-and-research-slice-variants"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19"
---

# Slice Research Subagent Assignment Manager

## Overview
This skill declares who owns each bounded research pass in a Research To Durable Knowledge Slice. Its core rule is assignment before collection: from an approved research plan, source map, evidence plan, and task decomposition, create an auditable `subagent-assignments.md` contract that partitions sources, limits authority, defines report shape, and explains how returned source material flows into later evidence and claim-ledger steps.

## When to Use
Use this after `research-questions.md`, `source-map.md`, `evidence-plan.md`, and `task-decomposition.md` exist for `slice.research-to-durable-knowledge`, and the next manifest gate is `subagent_assignments_declared`.

Use it when research tasks need explicit owners or subagent/pass labels, source partitions, non-overlap rules, isolation boundaries, evidence-return expectations, merge points, and fan-in rules before any worker or parallel pass starts collecting evidence.

Do not use it to frame questions, map sources, design the evidence plan, decompose tasks, collect or ingest evidence, label provenance/freshness, extract or decide claims, build findings, model durable objects, write KB seeds, approve promotion, update indexes, preserve a custom process, debug failures, execute operations, run external code or installers, or answer a simple query. No claim extraction, no synthesis, no procedure capture.

## Source Contract
Ground this skill in `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#procedure-and-research-slice-variants`, `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#research-to-durable-kb-slice-variant`, `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6`, `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19`, and `docs/architecture/master-plugin-target-architecture-capability-atom-shard-map.html#pipeline.slice.research_to_durable_knowledge.subagent.assignment.manager`.

The owning manifest is `capabilities/pipelines/slice-variants/research-to-durable-knowledge.pipeline.json`, step `slice-research-subagent-assignment-manager`. The step is required, invokes `skill:slice-research-subagent-assignment-manager`, produces `subagent-assignments.md`, gates on `subagent_assignments_declared`, reaches `ready_for_next_step`, and fails by `stop_or_handoff`.

Atom anchors are `pipeline.slice.research-to-durable-knowledge`, `pipeline.slice.research_to_durable_knowledge.subagent.assignment.manager`, and `capabilities/pipelines/slice-variants/research-to-durable-knowledge.pipeline.json#step_graph.steps.slice-research-subagent-assignment-manager`. External reference inputs are empty for this record.

## Operating Procedure
1. Verify prerequisites. Require a selected Research To Durable Knowledge Slice, the parent context, approved research plan or brief, research questions, source map, evidence plan, task decomposition, authority state, sensitivity limits, freshness requirement, and any required source-access notes. If tasks are not decomposed, route back to `slice-research-task-decomposer`.
2. Convert decomposed tasks into assignment units. Each unit must name one owner or pass, the research question or task ids it covers, the exact source partition, exclusions, dependencies, allowed read methods, expected evidence classes, and the next fan-in receiver.
3. Enforce non-overlap. No two assignments may claim the same source partition, claim family, or output path without an explicit coordination rule. Shared sources must have a primary owner, secondary reviewer, conflict protocol, and citation namespace.
4. Set isolation boundaries. State what each assignee may read, what it must not touch, which paths or URLs are restricted, which live/runtime sources need separate approval, and which findings require human review before inclusion.
5. Ban unsafe research execution. Every assignment must say that researched code, installers, package scripts, migrations, deploy commands, live probes, destructive commands, and external automation are out of scope unless a separate authorized operations Slice owns that action.
6. Define report shape. Require each subagent or pass to return a compact report with assignment id, sources inspected, citations and locators, collection timestamps, source freshness posture, confidence, sensitivity handling, negative findings, contradictions, gaps, blocked sources, and proposed evidence-corpus entries.
7. Define evidence-return contracts. Returned material must be source evidence or gap records, not accepted claims or durable truth. It must feed `evidence-corpus/index.md`, `evidence-log.md`, later provenance/freshness labeling, claim processing, contradiction review, negative knowledge, and downstream finding work without bypassing those steps.
8. Define merge and handoff expectations. Record fan-in order, deduplication owner, conflict resolver, cross-assignment dependency checks, expected subagent report paths such as `subagent-reports/`, and the condition for moving to evidence collection.
9. Write `subagent-assignments.md`. Include assignment table, scope boundaries, non-overlap matrix, report template, evidence-return fields, authority/sensitivity rules, no-execution statement, fan-in protocol, unresolved blockers, and next owner.
10. Close only when every decomposed task is assigned or explicitly blocked. Use `ready_for_next_step` when `subagent_assignments_declared` is satisfied; otherwise use `stop_or_handoff` with named missing tasks, source ambiguity, authority limits, overlap, sensitivity, unavailable assignees, or unresolved fan-in risks.

## Outputs
The required output is `subagent-assignments.md`. It must contain assignment ids, owners or pass labels, task and question coverage, source partitions, exclusions, dependencies, allowed read methods, isolation and no-execution boundaries, report shape, evidence-return fields, fan-in owners, merge expectations, non-overlap checks, sensitivity notes, freshness expectations, blocked assignments, and terminal-state recommendation.

Optional slice-local outputs may include a `subagent-reports/` path plan, assignment status table, or handoff note. These are assignment contracts only; they are not evidence collection, evidence ingestion, claim ledger rows, downstream finding artifacts, durable-KB seed content, promotion edges, index updates, canonical KB writes, process drafts, debug findings, or execution logs.

## Verification
Verify the gate `subagent_assignments_declared`: every task in `task-decomposition.md` is assigned to exactly one owner or explicitly blocked, every planned source class has a source partition and allowed read method, and every overlap has a named coordination rule.

Audit `subagent-assignments.md` for report templates, evidence-return fields, citation requirements, freshness/provenance expectations, sensitivity handling, no-execution language, fan-in order, conflict handling, and handoff to evidence collection. Confirm it does not collect evidence itself, accept claims, build findings, write durable KB content, approve promotion, or authorize source/workspace mutation.

Validate the implementation with `node tools/validate-internal-skill-body-quality.mjs --skill slice-research-subagent-assignment-manager` and trigger coverage with `node tools/validate-internal-skill-trigger-fixtures.mjs --skill slice-research-subagent-assignment-manager`. Also parse the two owned fixture JSON files, check that the skill has exactly the seven Layer 6B H2 sections, and run `git diff --check` scoped to this skill and its fixtures.

## Failure Modes
Stop or hand off when the Slice is not the research-to-durable variant, prerequisite artifacts are missing, task decomposition is ambiguous, source partitions overlap without an owner, assignment ownership is unknown, source access needs approval, sensitive or restricted sources cannot be described safely, a task requires live probing or external code execution, or fan-in expectations are too vague to audit.

Block progression when any assignment lacks an owner, source limit, report shape, evidence-return contract, no-execution boundary, citation expectation, freshness posture, or merge path. Keep zero direct dispatch, evidence, durable-write, promotion, or execution authority here. No claim extraction, no synthesis, no procedure capture. Do not let assignment notes become evidence, claims, durable knowledge, promotion, debug conclusion, operational execution, or simple query response.

Route elsewhere when the actual work is question framing, source-map design, evidence-plan design, task decomposition, evidence collection, ingestion, provenance/freshness labeling, claim processing, contradiction classification, negative-knowledge capture, finding construction, durable object modeling, KB seed proposal, promotion, result writing, custom-process preservation, root-cause debugging, operations, deployment, or current-state query answering.
