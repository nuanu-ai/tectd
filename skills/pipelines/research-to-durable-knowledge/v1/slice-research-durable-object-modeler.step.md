---
id: "slice-research-durable-object-modeler"
kind: "internal_step"
owner_family: "slice_variant"
pipeline_id: "slice.research-to-durable-knowledge"
step_id: "slice-research-durable-object-modeler"
entry_gate: false
public_trigger: false
native_skill_discovery: false
body_path: "capabilities/instructions/pipelines/slice-variants/research-to-durable-knowledge/slice-research-durable-object-modeler.step.md"
source_manifest: "capabilities/pipelines/slice-variants/research-to-durable-knowledge.pipeline.json"
legacy_skill_ref: "slice-research-durable-object-modeler"
architecture_refs:
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#research-to-durable-kb-slice-variant"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19"
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#procedure-and-research-slice-variants"
---

# Research Durable Object Modeler

## Overview
This skill turns a completed claim ledger and synthesis into a proposed durable object model for the Research To Durable Knowledge Slice. Its core rule is proposal only: model candidate durable knowledge objects, lifecycle posture, and routing boundaries, but do not write canonical KB, update indexes, accept promotion, or create truth unsupported by the ledger.

## When to Use
Use this after source mapping, evidence collection, provenance/freshness labeling, claim-ledger completion, contradiction/gap classification, negative knowledge capture, and synthesis are available. Select it when the next decision is what durable object shapes should receive the research: KB page, object page, runbook, protocol note, index/front-door proposal, issue, follow-up Scope, or durable-domain handoff.

Do not use it to collect new evidence, extract claims, resolve contradictions, write the final KB seed prose, run a promotion gate, update indexes, or mutate durable knowledge. If the claim ledger or synthesis is missing, route back to the owning upstream research step.

## Source Contract
Ground this skill in `capabilities/pipelines/slice-variants/research-to-durable-knowledge.pipeline.json#step_graph.steps.slice-research-durable-object-modeler`, `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#research-to-durable-kb-slice-variant`, `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#procedure-and-research-slice-variants`, `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6`, `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19`, and the atom rows `pipeline.slice.research_to_durable_knowledge.durable.object.modeler` plus `pipeline.slice.research-to-durable-knowledge`.

The manifest step is required, invokes `slice-research-durable-object-modeler`, produces `durable-object-model.md`, gates on `durable_object_model_proposed_only`, reaches `ready_for_next_step`, and stops or hands off on failure. The next seed-writer step alone owns `durable-kb-seed.md`. External reference inputs for this record are empty.

## Operating Procedure
1. Confirm the input packet contains `claim-ledger.md`, `contradictions-and-gaps.md`, `negative-knowledge.md`, `synthesis.md`, source/provenance/freshness labels, and the parent Slice/Result identity. If any are absent, stop with the missing artifact named.
2. Group source claims by durable target intent: factual finding, object/current-state page, procedure/runbook candidate, protocol behavior, product or operations research, index/front-door discoverability, issue/backlog item, or follow-up Scope. Keep rejected, stale, source-only, contradicted, and restricted claims visible instead of folding them into accepted objects.
3. For each candidate object, declare object type, owner domain, proposed canonical location or handoff domain, and whether the research Slice may only create a projection/candidate. Distinguish canonical durable storage owned by Layer 5 stateful-domain pipelines from Slice-local proposal artifacts such as `durable-object-model.md`, the later `durable-kb-seed.md`, `object-pages-draft/`, `runbook-draft.md`, and `index-update-proposal.md`.
4. Define the object field model: title, purpose, owner, scope, included claims, excluded claims, source/proof links, evidence posture, provenance class, freshness class, authority limits, contradiction or gap status, negative knowledge links, maintenance/freshness trigger, merge/update target, lifecycle state, and downstream promotion prerequisite.
5. Attach every modeled field to claim IDs and evidence links from the ledger. Do not invent fields from synthesis language unless a source-backed claim supports them; mark unsupported implications as gaps, hypotheses, or follow-up candidates.
6. Choose the merge/update target for each candidate: new durable object proposal, update to an existing object, runbook-library proposal, protocol-knowledge route, product/operations research record, index/front-door proposal, issue/backlog item, follow-up Scope, or durable-domain handoff.
7. Assign lifecycle state and promotion posture: candidate, update-proposal, blocked-by-source, blocked-by-freshness, blocked-by-contradiction, restricted-review, rejected-insufficient-evidence, ready-for-KB-seed-proposal, or route-to-domain-owner. The lifecycle state is not acceptance of truth.
8. Prepare the handoff packet for the KB seed proposal writer: ordered candidate objects, field models, source claims, proof links, evidence posture, freshness and contradiction status, merge/update target, lifecycle state, promotion prerequisites, deferred items, and explicit no-write boundary.

## Outputs
The output is `durable-object-model.md`, consumed by the later `durable-kb-seed.md` writer and optional drafts such as `object-pages-draft/`, `runbook-draft.md`, or `index-update-proposal.md`. Each candidate object must include object type, owner domain, canonical versus projection boundary, field list, source claims, claim and proof links, evidence posture, freshness status, contradiction/gap status, negative knowledge links, merge/update target, lifecycle state, promotion prerequisites, and next route. A successful proposal includes exact line `gate: durable_object_model_proposed_only` and does not write the seed.

Minimum object row fields:
- `object_type`, `owner_domain`, proposed title, and scope.
- `source_claims`, evidence posture, proof links, freshness status, and contradiction/gap status.
- `merge_update_target`, lifecycle state, promotion prerequisites, and routing posture.

Valid terminal posture is `ready_for_next_step` only when `durable_object_model_proposed_only` is satisfied. Terminal states inside the artifact may be proposal-ready, blocked, rejected, restricted-review, handoff, or route-to-domain-owner. The model may recommend a seed proposal, human review, deferred work, or escalation to a durable-domain pipeline; it must not accept promotion or perform durable writes.

## Verification
Check that every proposed object traces to claim-ledger IDs and evidence/provenance/freshness records, and that each contradiction, gap, restricted source, stale claim, and negative finding remains visible. Verify each object has object type, owner domain, evidence posture, merge/update target, lifecycle state, routing posture, and promotion prerequisites. Verify the body references the owning manifest and architecture paths above, keeps exactly the seven Layer 6B sections, and contains no wrapper-only headings.

Validate behavior with scenarios where synthesis sounds convincing but lacks claim proof, where one object should be an index proposal rather than canonical KB, where a runbook candidate belongs to runbook-library promotion, and where contradictions block promotion. The correct result is a proposal-only object model with explicit routes and blockers.

## Failure Modes
Stop or hand off when claim ledger, synthesis, source links, provenance, freshness, contradiction review, object ownership, or promotion prerequisites are missing from the research packet. Block promotion posture when evidence is stale, authority is absent, source material is restricted, contradictions are unresolved, or the owner domain is unclear; categorize that case as proposal blocked rather than durable truth.

Route elsewhere when the task is evidence collection, claim extraction, synthesis, KB seed prose writing, promotion gate evaluation, index/front-door update, procedure capture, or stateful-domain canonical writing. Never mutate durable KB, update indexes, execute researched material, hide rejected findings, merge raw evidence into canonical truth, or convert a useful research result into an accepted durable object without proof and promotion authority.

If candidate targets overlap across domains, keep the conflict explicit and hand off with the competing owner domains, affected claim IDs, and proof needed to choose safely.
