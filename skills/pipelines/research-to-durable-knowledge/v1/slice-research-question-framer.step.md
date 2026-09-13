---
id: "slice-research-question-framer"
kind: "internal_step"
owner_family: "slice_variant"
pipeline_id: "slice.research-to-durable-knowledge"
step_id: "slice-research-question-framer"
entry_gate: false
public_trigger: false
native_skill_discovery: false
body_path: "capabilities/instructions/pipelines/slice-variants/research-to-durable-knowledge/slice-research-question-framer.step.md"
source_manifest: "capabilities/pipelines/slice-variants/research-to-durable-knowledge.pipeline.json"
legacy_skill_ref: "slice-research-question-framer"
architecture_refs:
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#research-to-durable-kb-slice-variant"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19"
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#procedure-and-research-slice-variants"
---

# Slice Research Question Framer

## Overview
This skill turns an existing research contract into the question set that drives a Research To Durable Knowledge Slice. Its core rule is to make the scope, assumptions, unknowns, decision needs, acceptance criteria, exclusions, confidence thresholds, evidence needs, and downstream source-map cues explicit before anyone maps sources or gathers evidence.

## When to Use
Use this skill after `slice-research-kb-contract-writer` has produced or confirmed `research-brief.md` and the current gate is `questions_framed` for `slice.research-to-durable-knowledge`.

Use it when the Slice needs `research-questions.md` to clarify what must be learned, which decision each question supports, which claims need stronger proof, which assumptions still need validation, which areas are out of scope, what acceptance criteria define an answerable question set, and what confidence level is acceptable for later synthesis or promotion-candidate work.

Do not use it to write the research contract, build `source-map.md`, create `evidence-plan.md`, collect or ingest evidence, label provenance/freshness, extract claims, build the claim ledger, classify contradictions, synthesize findings, write `durable-kb-seed.md`, run a promotion gate, update an index, extract a repeatable procedure, diagnose a failure, answer a direct lookup request, or write canonical durable KB content.

## Source Contract
Grounding sources are `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#research-to-durable-kb-slice-variant`, `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#procedure-and-research-slice-variants`, `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6`, `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19`, `docs/architecture/master-plugin-target-architecture-capability-atom-shard-map.html#pipeline.slice.research-to-durable-knowledge`, and `docs/architecture/master-plugin-target-architecture-capability-atom-shard-map.html#pipeline.slice.research_to_durable_knowledge.question.framer`.

The owning manifest is `capabilities/pipelines/slice-variants/research-to-durable-knowledge.pipeline.json`, step `slice-research-question-framer`. The step is required, invokes `skill:slice-research-question-framer`, produces `research-questions.md`, gates on `questions_framed`, reaches `ready_for_next_step`, and fails by `stop_or_handoff`.

Architecture and mapping anchors are `pipeline.slice.research-to-durable-knowledge`, `pipeline.slice.research_to_durable_knowledge.question.framer`, and `capabilities/pipelines/slice-variants/research-to-durable-knowledge.pipeline.json#step_graph.steps.slice-research-question-framer`. External references are empty for this step. Durable knowledge remains proposal-only here; source mapping, evidence planning, collection, claim extraction, synthesis, seed writing, promotion, and durable-domain writes belong to later steps or other owners.

## Operating Procedure
1. Confirm the active Slice is the Research To Durable Knowledge variant, `research-brief.md` exists or explicitly blocks, and the next manifest step is `slice-research-question-framer`. If the brief is missing or cannot state scope, durable lane candidate, authority, freshness, or proof policy, stop or hand off to the contract writer.
2. Extract the contract boundaries into a working frame: research purpose, target durable lane candidate, intended consumers, source classes, known constraints, promotion prerequisites, freshness requirement, authority posture, open blockers, and non-goals.
3. Convert the purpose into question clusters. Use separate clusters for decision questions, factual inventory questions, comparison or landscape questions, source-authority questions, freshness questions, risk and contradiction questions, and promotion-readiness questions when they apply.
4. For every question, state why it matters: the downstream decision it unlocks, the claim or object field it may support, the required proof class, the minimum acceptable confidence, the acceptance criteria for treating the question as answered, and the condition that would make the answer insufficient.
5. Add exclusions and guardrails. Mark questions that are intentionally out of scope, questions that would require live execution or restricted material, and questions that must be routed to diagnostic, repeatable-procedure, query-service, or durable-domain owners instead of this research Slice.
6. Turn uncertainty into downstream cues without doing downstream work: likely source classes, source-map hints, evidence-plan concerns, freshness checks, authority checks, contradiction probes, negative-knowledge candidates, and assumptions that need user or owner resolution.
7. Preserve ambiguity instead of smoothing it over. If the contract implies competing interpretations, stale inputs, missing source scope, or conflicting authority, write explicit unresolved questions and the handoff needed to resolve them.
8. Write `research-questions.md` with clusters, scope, assumptions, decision links, acceptance criteria, exclusions, confidence levels, source-map and evidence-plan cues, unresolved assumptions, blocked items, and the next step. Use `ready_for_next_step` only when the questions are specific enough for `slice-research-source-map-builder`; otherwise use `stop_or_handoff`.

## Outputs
Primary output: `research-questions.md`.

Required content: selected variant `slice.research-to-durable-knowledge`, reference to the source `research-brief.md`, grouped research questions, scope, assumptions, decision needs, acceptance criteria, exclusions, non-goals, expected confidence levels, proof expectations, evidence needs, source-map cues, evidence-plan cues, freshness and authority concerns, contradiction probes, missing information, and the terminal decision.

The output is a framing artifact only. It may point to likely source classes or evidence needs, but it must not map actual sources, collect evidence, assign subagents, extract claims, synthesize findings, draft KB seed prose, promote claims, mutate indexes, write durable knowledge, extract procedures, diagnose failures, or answer direct lookup requests.

## Verification
Verify that `research-questions.md` satisfies `questions_framed`: each major question ties back to the research contract, states the decision or claim field it supports, names assumptions and exclusions, defines acceptance criteria plus acceptable confidence or proof posture, and gives enough cues for source-map and evidence-plan builders to proceed without guessing.

Verify that the body keeps exactly the seven Layer 6B sections, references at least one `docs/architecture/*.html` source, references `capabilities/pipelines/slice-variants/research-to-durable-knowledge.pipeline.json`, and keeps the step tied to `slice-research-question-framer`.

Verify trigger behavior with positive cases where a contract already exists and the next need is question framing, and negative cases where the user needs contract writing, source mapping, evidence planning, collection, claim extraction, synthesis, KB seed writing, promotion, durable writes, repeatable-procedure extraction, failure diagnosis, or a direct lookup response.

## Failure Modes
Stop or hand off when `research-brief.md` is absent, stale, contradictory, or too vague to frame questions; when the target durable lane, authority state, freshness requirement, source classes, or promotion prerequisites are missing; or when the required answer depends on restricted material that cannot be safely summarized.

Route away when the request is actually a contract-writing task, a source-map or evidence-plan task, evidence collection, claim ledger work, contradiction classification, synthesis, durable KB seed drafting, promotion, canonical durable write, repeatable-procedure extraction, failure diagnosis, operational execution, or direct lookup response.

Block progression when questions would silently assume current truth from raw sources, generated projections, old memory, untrusted external code, or unresolved contradictions. The correct terminal path is `stop_or_handoff`, `blocked_by_missing_sources`, `blocked_by_freshness_or_authority`, `blocked_by_contradiction`, or a routed next owner, not a vague question list.
