---
id: "slice-research-kb-contract-writer"
kind: "internal_step"
owner_family: "slice_variant"
pipeline_id: "slice.research-to-durable-knowledge"
step_id: "slice-research-kb-contract-writer"
entry_gate: false
public_trigger: false
native_skill_discovery: false
body_path: "capabilities/instructions/pipelines/slice-variants/research-to-durable-knowledge/slice-research-kb-contract-writer.step.md"
source_manifest: "capabilities/pipelines/slice-variants/research-to-durable-knowledge.pipeline.json"
legacy_skill_ref: "slice-research-kb-contract-writer"
architecture_refs:
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#research-to-durable-kb-slice-variant"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19"
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#procedure-and-research-slice-variants"
---

# Slice Research KB Contract Writer

## Overview
This skill writes the slice-local durable knowledge candidate contract for a Research To Durable Knowledge Slice. Its output is `research-brief.md`, a proposal-only contract that declares the research scope, source inputs, target durable lane candidate, object contract, field schema, evidence classes, proof and freshness rules, authority boundaries, promotion prerequisites, handoff routes, and terminal decision for the next step.

The core rule is that a research contract is not durable truth. This skill can define the candidate contract that later artifacts must satisfy, but it must not collect evidence, write KB seed prose, approve promotion, mutate front doors or indexes, perform canonical durable KB writes, or claim whole-research completion.

## When to Use
Use this skill only when all trigger conditions are true:

- Runtime selected `slice.research-to-durable-knowledge`.
- `slice-research-kb-entry-gate` produced `slice.md` or an equivalent research target declaration.
- `slice-research-kb-context-loader` produced `README.md` or explicitly recorded `context_loaded_or_declared_missing`.
- The current manifest step is `slice-research-kb-contract-writer`.
- The next required gate is `research_contract_written`, and `research-brief.md` does not yet satisfy that gate.

Use it when downstream workers need a durable KB candidate contract before framing research questions, mapping sources, planning evidence, assigning subagents, collecting sources, extracting claims, drafting `durable-kb-seed.md`, or running promotion review.

Do not use it for detailed question framing, source-map building, evidence-plan writing, evidence collection, source ingestion, subagent dispatch, claim extraction, claim-ledger building, contradiction classification, synthesis, KB seed proposal prose, promotion-edge writing, promotion approval, front-door/index mutation, canonical durable-domain writing, whole-research completion, simple KB query answering, procedure capture, debug/root-cause work, operational execution, deployment, cleanup, or live-system commands.

## Source Contract
Grounding sources are `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#research-to-durable-kb-slice-variant`, `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#procedure-and-research-slice-variants`, `docs/architecture/master-plugin-target-architecture-part-6c-stateful-domain-pipelines.html#durable-kb-pipeline`, `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6`, and `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19`.

The owning manifest is `capabilities/pipelines/slice-variants/research-to-durable-knowledge.pipeline.json`, step `slice-research-kb-contract-writer`. The step is required, invokes `skill:slice-research-kb-contract-writer`, produces `research-brief.md`, gates on `research_contract_written`, reaches `ready_for_next_step`, and fails by `stop_or_handoff`.

Architecture and mapping anchors are `pipeline.slice.research-to-durable-knowledge`, `pipeline.slice.research_to_durable_knowledge.kb.contract.writer`, `docs/architecture/master-plugin-target-architecture-capability-atom-shard-map.html#pipeline.slice.research_to_durable_knowledge.kb.contract.writer`, and `capabilities/pipelines/slice-variants/research-to-durable-knowledge.pipeline.json#step_graph.steps.slice-research-kb-contract-writer`.

Source inputs are the runtime manifest view, `slice.md`, context-loader `README.md` or missing-context declaration, parent Program/Scope/Slice/Result links, user research intent, durable KB map or index references already loaded, prior research pointers, authority constraints, source-class constraints, sensitivity restrictions, freshness expectations, and any existing target-owner hints. External reference inputs are empty for this step. Durable KB canonical storage is owned by the durable KB domain pipeline after authority, freshness, contradiction, provenance, canonical-write, promotion, and index/front-door gates.

## Operating Procedure
1. Confirm trigger state. Name the selected variant, manifest step, `research_contract_written` gate, previous artifacts (`slice.md` and `README.md` or missing-context declaration), and whether the step is allowed to proceed.
2. Inventory source inputs without collecting new evidence. List the parent object refs, target Slice id, current context sources already loaded, prior research refs, durable KB map/index refs, authority constraints, sensitivity restrictions, freshness expectations, and missing inputs.
3. Restate the research purpose as a durable knowledge candidate. Identify what decision, concept, product area, protocol behavior, procedure/runbook candidate, operating fact, or object family needs durable reuse, and why a simple query answer is insufficient.
4. Select the proposed target durable lane and candidate object contract without creating that durable object. State target domain, object/page/record type, target owner, intended consumers, allowed use, expected lifecycle, storage owner, relation to Program/Scope/Slice/Result, and wrong-domain alternatives.
5. Define the downstream field schema. Include claim shape, object field, evidence pointer, source role, provenance, collection timestamp or snapshot date, freshness requirement, authority or allowed-use label, sensitivity label, contradiction status, confidence, proof class, promotion condition, and field owner.
6. Declare artifact ownership. This step owns only `research-brief.md`; later steps own `research-questions.md`, `source-map.md`, `evidence-plan.md`, `task-decomposition.md`, `subagent-assignments.md`, `evidence-corpus/index.md`, `source-provenance.md`, `freshness-authority.md`, `claim-ledger.md`, `contradictions-and-gaps.md`, `negative-knowledge.md`, `synthesis.md`, `durable-kb-seed.md`, `promotion.md`, `deferred.md`, and `result.md`.
7. Set source, proof, and freshness rules. Raw sources are not durable truth, generated indexes are pointers, memory/session evidence is historical unless refreshed, restricted material stays labeled, live or time-sensitive claims require a refresh path, and contradicted, stale, source-only, blocked, or rejected findings must remain visible.
8. Record promotion prerequisites without approving promotion: evidence corpus, source map, provenance labels, freshness-authority labels, claim ledger, contradiction and gap review, negative knowledge, synthesis, proposed KB seed, promotion gate result, explicit target-owner authority, and index/front-door update proposal for a later owner.
9. Write `research-brief.md` as a candidate contract with this shape: `contract_status: proposal_only`, `target_durable_lane`, `candidate_object_contract`, `source_inputs`, `field_schema`, `artifact_ownership`, `source_proof_freshness_policy`, `authority_boundary`, `promotion_prerequisites`, `forbidden_actions`, `terminal_decision`, `handoff_routes`, and `next_step`.
10. Choose the step terminal decision. Use `ready_for_next_step` only when the brief satisfies `research_contract_written` and can hand off to `slice-research-question-framer`. Use `stop_or_handoff` when target lane, authority, source scope, freshness policy, parent context, sensitivity handling, or owner route is missing.

## Outputs
Primary output: `research-brief.md` inside the active Research To Durable Knowledge Slice.

Required output shape:

- Trigger state: selected variant, manifest step, previous artifact refs, and gate.
- Research scope: purpose, exclusions, simple-query rejection reason, and expected durable use.
- Source inputs: loaded context, missing context, parent refs, prior research refs, durable KB map/index refs, authority constraints, sensitivity constraints, and freshness expectations.
- Target durable lane candidate: domain, owner, intended consumers, allowed use, object/page/record type, lifecycle, and wrong-domain alternatives.
- Candidate object contract: field schema, field ownership, claim/evidence/provenance/freshness/authority/contradiction/proof requirements, and promotion condition.
- Artifact contract: required/optional downstream artifacts and the owner step for each class.
- Promotion prerequisites: claim ledger, contradiction review, negative knowledge, synthesis, KB seed proposal, promotion gate, target-owner authority, and index/front-door update proposal.
- Boundary block: no durable KB write, no canonical KB write, no promotion approval, no front-door/index mutation, no evidence collection, no KB seed prose, no whole-research completion.
- Terminal decision: `ready_for_next_step` or `stop_or_handoff`, with blockers, handoff route, and next step.

The output is a contract proposal only. It may name future durable objects, fields, storage lanes, promotion conditions, and handoff owners, but it must not draft `durable-kb-seed.md`, promote claims, mutate indexes, dispatch research workers, collect evidence, or write canonical durable knowledge.

## Verification
Verify that `research-brief.md` satisfies the `research_contract_written` proof gate:

- The trigger state names `slice.research-to-durable-knowledge`, `slice-research-kb-contract-writer`, `research_contract_written`, `research-brief.md`, and the previous context basis.
- Source inputs and missing inputs are explicit; no new evidence collection is claimed.
- The target durable lane, target owner, candidate object contract, field schema, evidence classes, source role, provenance, freshness, authority, contradiction policy, proof class, and promotion prerequisites are present.
- Artifact ownership separates this step from question framing, source mapping, evidence planning, evidence collection, claim extraction, synthesis, KB seed proposal, promotion, result writing, and durable-domain writing.
- The no-write boundary states no durable KB write, no canonical KB write, no promotion approval, no front-door/index mutation, no pipeline execution authorization, no live-system command authorization, and no whole-research completion.
- The terminal decision is `ready_for_next_step` with handoff to `slice-research-question-framer`, or `stop_or_handoff` with named missing source, authority, freshness, target-owner, parent-context, sensitivity, contradiction, or route information.

Validate this skill with `node tools/validate-internal-skill-body-quality.mjs --skill slice-research-kb-contract-writer` and `node tools/validate-internal-skill-trigger-fixtures.mjs --skill slice-research-kb-contract-writer`. Also parse both owned JSON fixtures, scan the owned files for trailing whitespace and final newline, and run scoped `git diff --check` for the owned files.

## Failure Modes
Use `stop_or_handoff` when the active target is not a Research To Durable Knowledge Slice, entry/context artifacts are missing, the target cannot be tied to a parent Program/Scope/Slice/Result, durable lane candidate is unclear, target owner is unknown, source scope is missing, freshness rules are absent, authority or allowed use cannot be stated, sensitivity restrictions block safe summary, or unresolved contradiction prevents a coherent contract.

Route to `slice-research-kb-entry-gate` or `slice-research-kb-context-loader` when entry or context state is missing. Route to `slice-research-question-framer`, `slice-research-source-map-builder`, `slice-research-evidence-plan-builder`, or later research steps when their artifacts are the actual requested work. Route to the durable KB domain pipeline only for canonical write eligibility or promotion-owner work. Route to query services for direct answers, to `slice.custom-procedure-capture` for reusable procedure capture, to `slice.debug-root-cause` for regressions or unknown failures, to Program/Scope/Slice decomposition for large feature discovery, and to human review when authority, sensitivity, or target ownership cannot be resolved by the agent.

Block progression when the requested output is a canonical KB page, KB seed prose, index/front-door mutation, promotion approval, live research execution, installer/code execution, source repo mutation, deployment, cleanup, procedure capture, debug investigation, direct query response, durable-domain write, or completion claim for the whole research Slice. Name the owner route instead of stretching this contract step.
