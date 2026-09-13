---
id: "slice-research-contradiction-and-gap-classifier"
kind: "internal_step"
owner_family: "slice_variant"
pipeline_id: "slice.research-to-durable-knowledge"
step_id: "slice-research-contradiction-and-gap-classifier"
entry_gate: false
public_trigger: false
native_skill_discovery: false
body_path: "capabilities/instructions/pipelines/slice-variants/research-to-durable-knowledge/slice-research-contradiction-and-gap-classifier.step.md"
source_manifest: "capabilities/pipelines/slice-variants/research-to-durable-knowledge.pipeline.json"
legacy_skill_ref: "slice-research-contradiction-and-gap-classifier"
architecture_refs:
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#research-to-durable-kb-slice-variant"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19"
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#procedure-and-research-slice-variants"
---

# Slice Research Contradiction And Gap Classifier

## Overview
This skill turns an existing research claim ledger into an auditable contradiction, gap, stale-evidence, unresolved-question, and blocked-claim map. The core rule is classification before interpretation: preserve every issue and route the next proof work without resolving contradictions, collecting new evidence, synthesizing findings, or promoting durable KB.

## When to Use
Use this when `claim-ledger.md` already exists for a Research To Durable Knowledge Slice and the next step is to classify ledgered claims before synthesis. The trigger is a populated claim ledger with claim IDs, evidence links or missing-link markers, provenance labels, freshness labels, source authority labels, negative-knowledge references, or visible weak and blocked rows.

Do not use this for raw source gathering, claim extraction, claim-ledger construction, contradiction resolution, research synthesis, durable-object modeling, seed KB writing, promotion gates, index or front-door updates, live/current-answer lookup, or durable-domain mutation. If no claim ledger exists, route upstream to claim extraction or ledger building. If the user asks which source is true now, route to query/freshness or the relevant evidence collector. If promotion is requested, require this classification artifact first and then route to the research promotion gate.

## Source Contract
Ground this skill in `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#research-to-durable-kb-slice-variant`, `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#procedure-and-research-slice-variants`, `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6`, and `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19`.

The owning manifest is `capabilities/pipelines/slice-variants/research-to-durable-knowledge.pipeline.json`, step `slice-research-contradiction-and-gap-classifier`. The step is required, invokes this skill, produces `contradictions-and-gaps.md`, gates on `contradictions_and_gaps_classified`, fails by `stop_or_handoff`, and can advance only as `ready_for_next_step`.

Required source inputs are the active research question set, `source-map.md`, `evidence-plan.md`, `evidence-corpus/index.md`, `evidence-log.md`, `source-provenance.md`, `freshness-authority.md`, `claim-ledger.md`, and `negative-knowledge.md` when present. Missing inputs are not permission to infer facts; record them as gaps or blockers. External references for this manifest step are empty.

## Operating Procedure
1. Confirm scope: the active Slice variant is `slice.research-to-durable-knowledge`, the requested action is classify-only, and `claim-ledger.md` is the claim source of record. Do not rewrite claim text except to quote the affected row.
2. Build a per-claim disposition matrix from `claim-ledger.md`. For every claim ID, record evidence IDs, source class, provenance label, freshness label, authority label, confidence, negative-knowledge link, allowed-use limit, and current synthesis eligibility.
3. Scan each claim for issue signals: competing claim text, incompatible numbers, incompatible dates, source-only facts, derived interpretation treated as fact, uncited claim, missing source class, stale evidence, weak authority, restricted-needed rows, media gaps, transcript gaps, privacy limits, unresolved questions, and blocked claims.
4. Assign stable issue handles. Use `CON-001` for contradictions, `GAP-001` for proof gaps, `STALE-001` for stale-evidence blockers when freshness is the primary issue, and `Q-001` for unresolved questions that cannot be answered from the ledger.
5. Classify contradictions by type: direct factual conflict, temporal conflict, definition or scope mismatch, authority conflict, interpretation conflict, provenance conflict, source-version drift, and claim-versus-negative-knowledge conflict. Do not choose a winning source unless the ledger already marks one source as authoritative and current.
6. Classify gaps by missing proof route: missing primary source, missing current/live verification, missing local repo or git evidence, missing runtime/log/receipt proof, missing official external authority, missing user or SME confirmation, missing transcript or media proof, restricted evidence unavailable, uncited claim, stale evidence, and insufficient provenance.
7. Classify blocked claims separately from ordinary weak support. Use handling states `classify_only`, `non_blocking_limit`, `needs_evidence_collection`, `needs_freshness_refresh`, `needs_authority_review`, `needs_human_review`, `blocked_by_missing_sources`, `blocked_by_freshness_or_authority`, `blocked_by_contradiction`, `deferred`, and `rejected_insufficient_evidence`.
8. Define next proof routes without performing them: source-map repair, evidence collector, provenance and freshness labeler, KB query, live/runtime verification, code or git inspection, transcript or media processing, human review, durable-domain escalation, or follow-up Slice. A proof route names the owner or next skill; it does not gather evidence in this step.
9. Reconcile counts before closing: every claim must be no-issue, linked to at least one issue handle, rejected, blocked, or deferred. Claims with any unresolved blocking handle are not synthesis-ready.
10. Produce the handoff. If all claims have dispositions and blocked work is visible, hand off to research synthesis. If blockers dominate, hand off upstream to proof work with exact missing source classes and blocked handles.

## Outputs
The required artifact is `contradictions-and-gaps.md`. It contains:

- scope and source-input summary;
- status totals by claim disposition and issue-handle type;
- contradiction table;
- proof-gap table;
- stale-evidence and authority-gap table when needed;
- unresolved-question and blocked-claim table;
- claim-to-handle cross references;
- blocked and deferred rows;
- next proof routes and handoff target.

Each contradiction row includes handle, type, claim IDs, competing assertions, evidence IDs, source authority comparison, freshness comparison, risk, handling state, downstream effect, and whether synthesis is blocked. Each gap row includes handle, claim IDs, missing source class, why the proof is required, stale or authority issue, allowed-use limit, next proof route, owner or route, and terminal recommendation.

Terminal states are `ready_for_next_step` when `contradictions_and_gaps_classified` is true and all unresolved issues are visible, or `stop_or_handoff` when missing sources, stale authority, restricted evidence, unstable claim IDs, or unresolved contradictions prevent a complete classification. The artifact may recommend `blocked_by_missing_sources`, `blocked_by_freshness_or_authority`, `blocked_by_contradiction`, `deferred`, or `rejected_insufficient_evidence` for individual rows.

## Verification
Verify that every claim in `claim-ledger.md` appears in exactly one category or has an explicit multi-handle explanation: no blocking issue, contradiction handle, gap handle, stale-evidence handle, unresolved-question handle, rejected, blocked, or deferred. Cross-check that every contradiction references at least two claim or evidence sources and every gap names the missing source class and next proof route.

Check that stale, uncited, source-only, restricted, weak-authority, unresolved-question, and media/transcript-dependent claims are not treated as synthesis-ready unless their limits are explicitly non-blocking. Confirm the output does not resolve contradictions, add new evidence, collect missing evidence, write synthesis.md, accept promotion, mutate durable KB, update indexes, execute researched code, or hide blocked work.

Proof commands for this skill are `node tools/validate-internal-skill-body-quality.mjs --skill slice-research-contradiction-and-gap-classifier`, `node tools/validate-internal-skill-trigger-fixtures.mjs --skill slice-research-contradiction-and-gap-classifier`, JSON parse of both owned fixtures, line-count and whitespace checks over the three owned files, and `git diff --check --` scoped to those files.

## Failure Modes
Block or hand off when `claim-ledger.md` is missing, claim IDs are unstable, evidence IDs cannot be traced, provenance or freshness labels are absent, source authority is unknowable, privacy restrictions prevent citation, restricted evidence is required, or the research question requires current truth that has not been refreshed. Record the exact missing source class and route rather than lowering confidence silently.

Route upstream to source mapping, evidence collection, provenance/freshness labeling, transcript/media processing, code/git inspection, or live verification when missing proof can be collected by another step. Route to human review when authority, SME judgment, or restricted-source access is needed. Route to query/freshness when current-state truth is the issue. Route to durable-domain escalation only as a proposed downstream path after classification.

Forbidden actions are evidence gathering, contradiction resolution, finding synthesis, durable KB promotion, seed KB writing, front-door or index updating, source repo mutation, live-system commands, and executing researched external code or installers. Do not convert a blocked claim into accepted truth because it is useful or likely.
