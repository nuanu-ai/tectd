---
id: "slice-research-task-decomposer"
kind: "internal_step"
owner_family: "slice_variant"
pipeline_id: "slice.research-to-durable-knowledge"
step_id: "slice-research-task-decomposer"
entry_gate: false
public_trigger: false
native_skill_discovery: false
body_path: "capabilities/instructions/pipelines/slice-variants/research-to-durable-knowledge/slice-research-task-decomposer.step.md"
source_manifest: "capabilities/pipelines/slice-variants/research-to-durable-knowledge.pipeline.json"
legacy_skill_ref: "slice-research-task-decomposer"
architecture_refs:
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#research-to-durable-kb-slice-variant"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19"
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#procedure-and-research-slice-variants"
---

# Research Task Decomposer

## Overview
This skill produces `task-decomposition.md` for the Research To Durable Knowledge Slice. Its core rule is to split planned research into bounded task units with source families, evidence goals, proof requirements, allowed tools, stop conditions, artifact expectations, and fan-in requirements before any worker assignment or evidence collection begins.

## When to Use
Use this after `research-questions.md`, `source-map.md`, and `evidence-plan.md` exist or their missing pieces are explicitly recorded. Select it when the next useful artifact is a task plan that turns questions and source classes into independently executable research units.

Do not use it to frame research questions, build the source map, create the evidence plan, assign subagents, collect or ingest evidence, label provenance/freshness, extract claims, synthesize findings, write `durable-kb-seed.md`, approve promotion, capture a procedure, debug a failure, or answer a simple query.

## Source Contract
The owning manifest step is `slice-research-task-decomposer` in `capabilities/pipelines/slice-variants/research-to-durable-knowledge.pipeline.json` for `slice.research-to-durable-knowledge`. Architecture grounding comes from `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#research-to-durable-kb-slice-variant`, `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#procedure-and-research-slice-variants`, `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6`, `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19`, and the atom row `pipeline.slice.research_to_durable_knowledge.task.decomposer`.

The step produces only `task-decomposition.md`, satisfies gate `tasks_decomposed`, and ends at `ready_for_next_step` or `stop_or_handoff`.

## Operating Procedure
1. Check prerequisites: parent Slice context, `research-questions.md`, `source-map.md`, `evidence-plan.md`, authority constraints, forbidden source/execution boundaries, and promotion target assumptions. If any prerequisite is missing, record the gap and route to the owning earlier step.
2. Map decomposition axes: question groups, decision needs, source families, evidence goals, freshness/authority checks, contradiction-prone areas, restricted/private sources, and optional artifact expectations such as transcripts, screenshots, raw sources, comparison matrix, object-page draft, runbook draft, or handoff note.
3. Build a task card for each bounded research unit. Include task ID, objective, covered questions, source/evidence partition, allowed tools or read methods, explicit exclusions, dependency list, expected artifacts, proof requirement, risk labels, stop condition, handoff shape, and fan-in target.
4. Keep task boundaries independent enough for later assignment. Prefer one worker or pass per source cluster, question family, or contradiction probe; split tasks that require unrelated source classes, different authority levels, or separate freshness checks.
5. Add dependency and ordering rules. Mark tasks that must precede others, tasks that can run in parallel, tasks that require human access or approval, and tasks that must stop when freshness, authority, privacy, restricted-source, or unsafe-execution limits are hit.
6. Define subagent assignment boundaries without assigning or dispatching subagents. State the report shape that downstream assignment should request, the allowed source limits, the fields required for later evidence ingestion, and the fan-in requirements needed before claim extraction or synthesis.
7. Confirm adjacent-step boundaries. The decomposition may name suggested task lanes, but it must not dispatch subagents, collect evidence, decide claim truth, synthesize findings, write a KB seed, promote durable knowledge, mutate indexes, or treat raw source material as current authority.
8. Close with a coverage scan: every research question and every source class is either mapped to at least one task or listed as an explicit gap, every task has a proof need and risk labels, and every fan-in point names the downstream artifact it supports.

## Outputs
Produce `task-decomposition.md` with these sections: input context, decomposition assumptions, task table, dependency/order map, source/evidence partitions, proof matrix, risk register, handoff/merge points, explicit gaps, and next-step readiness.

The task table should include task ID, objective, questions, source families, evidence goals, allowed tools or read methods, allowed evidence, exclusions, dependencies, expected artifact(s), proof requirement, risk labels, stop condition, handoff shape, fan-in target, and unresolved access or authority needs.

Artifact expectations are planning promises only: they name what a later assignee should return, such as a source list, transcript review note, screenshot inventory, comparison matrix, evidence-corpus candidate, contradiction note, or gap record. They are not collected evidence, accepted claims, synthesis, durable KB writes, promotion edges, or index updates.

The terminal result is `ready_for_next_step` only when the task set is bounded enough for `slice-research-subagent-assignment-manager` to assign work without redesigning the research plan. Otherwise return `stop_or_handoff` with the missing question, source, proof, or authority boundary.

## Verification
Verify the body by checking that `task-decomposition.md` traces every research question and source-map class to a bounded task or explicit gap, preserves the evidence-plan proof standard, and names fan-in points for evidence ingestion, claim extraction, contradiction/gap review, synthesis, and promotion review. Confirm no task requires broad unspecific reading, hidden live execution, installer execution, durable KB writes, or index/front-door mutation.

Audit the decomposition as a contract for the next step: each task must have source/evidence partitions, proof needs, allowed tools or read methods, artifact expectations, stop conditions, and fan-in requirements. The `tasks_decomposed` gate is not satisfied if the next step would need to redesign task scope, infer evidence goals, invent handoff shapes, or decide source authority from scratch.

Run `node tools/validate-internal-skill-body-quality.mjs --skill slice-research-task-decomposer` and `node tools/validate-internal-skill-trigger-fixtures.mjs --skill slice-research-task-decomposer` after editing this skill.

## Failure Modes
Stop or hand off when research questions, source-map, or evidence-plan context is missing; when the task set cannot be bounded without new scoping decisions; when source authority, privacy, freshness, or unsafe-execution constraints make partitions ambiguous; or when the request actually belongs to adjacent steps such as subagent assignment, evidence collection, synthesis, KB seed writing, promotion, procedure capture, debug/root-cause, durable domain writes, or simple query answering.

Record blockers as explicit gaps rather than filling them with assumed tasks. If a dependency cycle appears, split the task or route back to evidence planning. If allowed tools, proof requirements, artifact expectations, or fan-in requirements cannot be named from the existing inputs, return `stop_or_handoff` rather than inventing them.

If the user asks for direct research work during this step, produce the decomposition first and leave execution to the downstream assignment and evidence steps.

Use this boundary zone for unresolved decomposition issues; do not convert them into assumed research work.
