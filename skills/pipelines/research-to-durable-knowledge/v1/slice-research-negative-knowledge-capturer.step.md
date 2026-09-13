---
id: "slice-research-negative-knowledge-capturer"
kind: "internal_step"
owner_family: "slice_variant"
pipeline_id: "slice.research-to-durable-knowledge"
step_id: "slice-research-negative-knowledge-capturer"
entry_gate: false
public_trigger: false
native_skill_discovery: false
body_path: "capabilities/instructions/pipelines/slice-variants/research-to-durable-knowledge/slice-research-negative-knowledge-capturer.step.md"
source_manifest: "capabilities/pipelines/slice-variants/research-to-durable-knowledge.pipeline.json"
legacy_skill_ref: "slice-research-negative-knowledge-capturer"
architecture_refs:
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#research-to-durable-kb-slice-variant"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19"
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#procedure-and-research-slice-variants"
---

# Negative Knowledge Capturer

## Overview

This skill records research evidence that narrows what must not be trusted, reused, or promoted. Its core rule is: a disproven claim, failed search, dead end, stale doc, unsafe source, source exclusion, or non-working command is useful research output, but it is negative knowledge, not a positive KB claim.

## When to Use

Use after the research Slice has a source map, evidence plan, evidence corpus, source provenance, freshness/authority labels, and a claim ledger or explicit gap record. The step sits after `slice-research-claim-ledger-builder` and before contradiction/gap classification, synthesis, durable object modeling, KB seed writing, and promotion gates.

Use for disproven or contradicted claims, searched-but-absent evidence, dead-end research paths, stale docs, non-working commands, version-mismatched examples, inaccessible sources, source exclusions, unsafe or restricted sources, source-only facts that cannot support current truth, rejected hypotheses, blocked proof, non-publishable findings, out-of-scope discoveries, or information not worth durable capture.

Do not use for raw evidence collection, source ingestion, provenance labeling, positive claim extraction, full contradiction classification, synthesis, durable object modeling, KB seed proposals, promotion approval, promotion-edge writing, index/front-door updates, simple fact lookup, debug/root-cause discovery, procedure capture, or durable-domain writes.

## Source Contract

Grounding sources:

- `capabilities/pipelines/slice-variants/research-to-durable-knowledge.pipeline.json` step `slice-research-negative-knowledge-capturer`
- `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#research-to-durable-kb-slice-variant`
- `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#procedure-and-research-slice-variants`
- `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6`
- `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19`

The manifest step is required, invokes `slice-research-negative-knowledge-capturer`, produces `negative-knowledge.md`, gates on `negative_knowledge_recorded`, fails through `stop_or_handoff`, and can only advance as `ready_for_next_step` after the no-promotion artifact is explicit. This skill preserves research-to-durable-KB reconciliation material; it does not execute research targets, mutate durable KB storage, approve promotion, or update indexes/front doors.

## Operating Procedure

1. Confirm the active Slice is `slice.research-to-durable-knowledge`, the artifact set includes the source/provenance/freshness and claim-ledger inputs, and the requested action is negative capture rather than synthesis, KB seeding, promotion, or index update.
2. Review `source-map.md`, `evidence-plan.md`, `evidence-corpus/index.md`, `evidence-log.md`, `source-provenance.md`, `freshness-authority.md`, `claim-ledger.md`, and any existing `contradictions-and-gaps.md` draft for negative candidates. Treat missing prerequisite artifacts as a gap, not permission to invent findings.
3. Capture each negative candidate at the narrowest truthful scope. Record the query terms, paths, repo or workspace, transcript/session/log reference, web source, runtime snapshot, owner, timestamp or freshness posture, inspection depth, and whether the source was inspected, attempted, excluded, or unavailable.
4. Classify the negative type with concrete labels: `disproven_claim`, `absent_in_scope`, `dead_end`, `non_working_command`, `stale_docs`, `version_mismatch`, `contradicted`, `rejected_hypothesis`, `too_stale`, `inaccessible`, `unsafe_to_use`, `restricted`, `source_exclusion`, `outside_scope`, `insufficient_evidence`, `blocked_missing_authority`, `non_publishable`, or `not_worth_durable_capture`.
5. Preserve source exclusions explicitly. For each excluded source, state the exclusion reason, safety label, freshness label, allowed-use label, affected claim/question, and whether the source can be revisited by a later gate or owner.
6. Preserve contradictions without resolving the whole contradiction ledger. Cross-reference the affected `claim-ledger.md` row or research question, state the opposing evidence, and route final contradiction handling to `slice-research-contradiction-and-gap-classifier`.
7. Write `negative-knowledge.md` with one record per entry. Each record must contain `id`, `negative_type`, `affected_claim_or_question`, `source_or_search_scope`, `provenance_ref`, `freshness_label`, `safety_or_restriction_label`, `evidence_or_attempted_access`, `confidence`, `limits`, `revisit_trigger`, `downstream_route`, and `promotion_status: not_a_durable_kb_seed`.
8. Add a reviewed-scope section even when no entries exist. It must name the reviewed artifacts, source classes, search scopes, and confidence behind the empty result. An empty artifact with no reviewed scope does not satisfy the gate.
9. Set `negative_knowledge_recorded` only when every material negative candidate has an entry or an explicit stop/handoff route. Emit `ready_for_next_step` only for contradiction/gap classification, synthesis, deferred notes, or result context; use `stop_or_handoff` when the artifact cannot truthfully preserve the negative finding.

## Outputs

Primary output is `negative-knowledge.md`, a no-promotion Slice artifact with this shape:

- `## Reviewed Scope`: artifacts read, source classes checked, search scopes, timestamps/freshness posture, and known gaps.
- `## Negative Findings`: one `NK-*` record per disproven claim, absence, dead end, stale doc, non-working command, unsafe source, rejected hypothesis, contradiction signal, blocked proof, or low-value finding.
- `## Source Exclusions`: excluded sources with exclusion reason, safety/freshness labels, allowed-use limits, owner or revisit route, and affected claim/question.
- `## Terminal Route`: `negative_knowledge_recorded`, `ready_for_next_step`, or `stop_or_handoff` with reason.

The artifact may feed `contradictions-and-gaps.md`, `synthesis.md`, `deferred.md`, or `result.md` as context. It must not become `durable-kb-seed.md`, a positive claim, a promoted finding, an index/front-door update, or a durable-domain write by itself.

## Verification

Verify the body with `node tools/validate-internal-skill-body-quality.mjs --skill slice-research-negative-knowledge-capturer` and trigger coverage with `node tools/validate-internal-skill-trigger-fixtures.mjs --skill slice-research-negative-knowledge-capturer`.

Content verification checks that the skill names `negative-knowledge.md`, `negative_knowledge_recorded`, `ready_for_next_step`, `stop_or_handoff`, reviewed source/search scope, provenance refs, freshness labels, safety labels, disproven claims, dead ends, stale docs, non-working commands, contradictions, source exclusions, rejected hypotheses, inaccessible or unsafe sources, outside-scope or low-value findings, confidence, limits, revisit triggers, and `promotion_status: not_a_durable_kb_seed`. Also verify it blocks research execution, durable KB mutation, promotion authority, index/front-door changes, deployment, live commands, source mutation, and broad absence claims without a searched scope.

## Failure Modes

Use `stop_or_handoff` when the source/search scope is unknown, provenance or freshness labels are missing, the claim ledger does not exist and no gap is declared, the negative finding depends on restricted material without approval, a source exclusion lacks safety/freshness basis, or an inaccessible source is being treated as proof of absence.

Block rather than continue if the artifact would hide contradicted or stale evidence, convert negative evidence into a positive KB claim, make broad absence claims from a narrow search, expose unsafe/private material, or collapse contradiction classification, synthesis, seed writing, promotion, or index/front-door checks into this step.

If no negative candidates exist, still record the reviewed scope and confidence. An empty `negative-knowledge.md` without reviewed scope does not satisfy `negative_knowledge_recorded`.

A zero-promotion rule applies: this step may preserve reasons not to trust or promote material, but it cannot convert those reasons into durable KB truth, positive claims, approval, promotion edges, durable-domain writes, or index changes.
