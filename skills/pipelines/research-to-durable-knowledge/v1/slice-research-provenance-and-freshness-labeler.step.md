---
id: "slice-research-provenance-and-freshness-labeler"
kind: "internal_step"
owner_family: "slice_variant"
pipeline_id: "slice.research-to-durable-knowledge"
step_id: "slice-research-provenance-and-freshness-labeler"
entry_gate: false
public_trigger: false
native_skill_discovery: false
body_path: "capabilities/instructions/pipelines/slice-variants/research-to-durable-knowledge/slice-research-provenance-and-freshness-labeler.step.md"
source_manifest: "capabilities/pipelines/slice-variants/research-to-durable-knowledge.pipeline.json"
legacy_skill_ref: "slice-research-provenance-and-freshness-labeler"
architecture_refs:
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#research-to-durable-kb-slice-variant"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19"
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#procedure-and-research-slice-variants"
---

# Slice Research Provenance And Freshness Labeler

## Overview
This skill labels the trust posture of already-ingested research evidence for `slice.research-to-durable-knowledge`. Its core rule is evidence posture before interpretation: no source, excerpt, snapshot, subagent report, generated projection, or derived note may feed claim extraction, synthesis, or promotion until provenance, freshness, authority, safety, and allowed-use limits are explicit.

The skill is executable as a standalone Tect body. It reads the active Slice research packet, labels each existing evidence unit, writes the two manifest-owned labeling artifacts, and either satisfies the `provenance_and_freshness_labeled` gate or stops with a named handoff route. It never collects new evidence and never writes canonical durable knowledge.

## When to Use
Use this only after `slice-research-evidence-ingester` has indexed collected material into `evidence-corpus/index.md` and `evidence-log.md`, and the selected manifest step is `slice-research-provenance-and-freshness-labeler`.

Use it when already-ingested evidence includes mixed local docs, code, git history, sessions, memories, transcripts, runtime snapshots, web pages, external repos, screenshots, generated summaries, or subagent reports that must be separated into source vs derived, current vs historical, live vs snapshot, authoritative vs advisory, restricted vs publishable, and stale-risk categories before claim extraction.

Do not use it to map source classes, plan or collect evidence, ingest raw material, browse for freshness, extract atomic claims, build claim-ledger statuses, classify contradictions, synthesize findings, model durable objects, write durable KB seeds, approve promotion, mutate durable knowledge, update indexes or front doors, capture procedures, debug root cause, or answer a simple query.

## Source Contract
Grounding sources are `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#research-to-durable-kb-slice-variant`, `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#procedure-and-research-slice-variants`, `docs/architecture/master-plugin-target-architecture-part-6c-stateful-domain-pipelines.html#durable-kb-pipeline`, `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6`, `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19`, and `docs/architecture/master-plugin-target-architecture-capability-atom-shard-map.html#pipeline.slice.research_to_durable_knowledge.provenance.and.freshness.labeler`.

The architecture text names the variant `slice.research-to-durable-kb`; the manifest and registry canonicalize the package implementation as `slice.research-to-durable-knowledge`. Treat them as the same research Slice boundary for this step.

The owning manifest is `capabilities/pipelines/slice-variants/research-to-durable-knowledge.pipeline.json`, step `slice-research-provenance-and-freshness-labeler`. The step is required, invokes `skill:slice-research-provenance-and-freshness-labeler`, produces `source-provenance.md` and `freshness-authority.md`, gates on `provenance_and_freshness_labeled`, reaches `ready_for_next_step`, and fails by `stop_or_handoff`.

Source inputs are the active Slice front door or runtime packet, `research-brief.md` or `research-questions.md`, `source-map.md`, `evidence-corpus/index.md`, `evidence-log.md`, collector notes, subagent reports, and any raw-source pointers referenced by the corpus. The Part 6C durable KB pipeline owns canonical durable truth after authority, freshness, contradiction, provenance, canonical-write, and index/front-door gates. This skill may label evidence for later use; it must not promote, store, or mutate canonical durable knowledge.

## Operating Procedure
1. Confirm the selected Slice variant and step. Continue only when the active packet names `slice.research-to-durable-knowledge`, `slice-research-provenance-and-freshness-labeler`, and the next gate `provenance_and_freshness_labeled`.
2. Load `research-brief.md` or `research-questions.md`, `source-map.md`, `evidence-corpus/index.md`, `evidence-log.md`, collector notes, subagent reports, and referenced raw-source pointers. If any evidence ID lacks a source-map link, locator, custody row, collection time, source owner, or safe-use boundary, stop with those exact IDs.
3. Build a labeling inventory from existing evidence only. Every evidence ID must become one of: labeled, duplicate-linked, restricted, unsafe, blocked, deferred, or rejected. Do not add new evidence, refresh sources, browse, execute installers or code, or infer missing source metadata from memory.
4. For each evidence unit, classify provenance with explicit labels such as `primary_source`, `official_source`, `repo_or_code_source`, `runtime_snapshot`, `session_or_memory_source`, `user_supplied_source`, `third_party_source`, `generated_summary`, `derived_analysis`, or `unknown_source`.
5. Mark the source and derivation relationship: raw source, snapshot, excerpt, paraphrase, generated projection, subagent summary, interpretation, duplicate alias, or downstream synthesis input. Generated, summarized, and derived items must point back to source evidence IDs and must not be treated as primary current truth.
6. Classify freshness independently from authority: `live_checked`, `current_at_collection`, `snapshot_only`, `historical`, `stale_risk`, `stale`, `superseded`, `undated`, or `needs_refresh`. Record the date, version, commit, timestamp, access time, snapshot marker, or missing-date reason.
7. Classify authority and allowed use independently from freshness: `authoritative_for_scope`, `advisory`, `supporting_context`, `requires_primary_source`, `restricted_use`, `unsafe_to_publish`, `needs_owner_review`, or `not_usable_for_claims`. A source may be authoritative but stale, current but advisory, primary but restricted, or derived and useful only for context.
8. Add safety, sensitivity, and scope limits. Note private source classes, quote limits, secret or credential risk, restricted custody, unsafe external code/installers, jurisdiction or product-scope limits, and whether the evidence can support current truth, historical context, comparison only, contradiction review, negative knowledge, or no claims.
9. Write `source-provenance.md` with an evidence-by-evidence table containing evidence ID, source-map ID, locator, provenance label, raw-or-derived status, derivation parent, source family, restriction status, safe citation form, and unlabeled or blocked reason.
10. Write `freshness-authority.md` with evidence ID, freshness label, timestamp or missing-date reason, authority label, allowed-use label, stale-risk status, refresh requirement, owner-review requirement, promotion block status, and next owner.
11. Close the gate only when every evidence ID is labeled or explicitly blocked, restricted, duplicate-linked, rejected, or deferred with a reason. Route blocked rows to the correct owner instead of fabricating labels.

## Outputs
Required outputs are `source-provenance.md` and `freshness-authority.md`.

`source-provenance.md` must include evidence IDs, source-map links, locators, source family, provenance label, raw-vs-derived status, derivation links, generated-summary parent links, duplicate aliases, source restrictions, safe citation form, and unlabeled or blocked reasons.

`freshness-authority.md` must include evidence IDs, freshness labels, source dates or missing-date reasons, authority labels, allowed-use limits, stale-risk or stale status, refresh requirements, owner-review requirements, evidence blocked from claim extraction, and evidence blocked from promotion.

The valid positive terminal posture is `ready_for_next_step` with `provenance_and_freshness_labeled` satisfied. Outputs may mark rows as unusable, stale, source-only, restricted, unsafe, duplicate-linked, rejected, deferred, or needing owner review. They must not create claim records, resolve contradictions, synthesize findings, approve promotion, write durable KB content, update indexes or front doors, perform fresh research, execute researched code, or claim the whole Slice is complete.

## Verification
Verify that every evidence ID in `evidence-corpus/index.md` or `evidence-log.md` appears in exactly one labeled, blocked, duplicate-linked, restricted, or deferred row across the two output artifacts. Check that generated summaries and subagent reports trace back to source evidence IDs and are never treated as primary source truth by themselves.

Verify freshness, authority, provenance, allowed use, and safety are separate labels. Confirm stale, undated, restricted, unsafe, weak-authority, generated-only, or untraceable evidence is blocked from promotion unless the limitation and next owner are explicit.

Verify the proof gate by checking that `source-provenance.md` and `freshness-authority.md` together prove `provenance_and_freshness_labeled`: every evidence unit is accounted for, every derived item links to source evidence, every blocked row names a reason, and no row claims canonical durable truth. Confirm Part 6C durable KB canonical-write and index/front-door gates remain unexecuted.

Validate the skill body with `node tools/validate-internal-skill-body-quality.mjs --skill slice-research-provenance-and-freshness-labeler` and trigger coverage with `node tools/validate-internal-skill-trigger-fixtures.mjs --skill slice-research-provenance-and-freshness-labeler`.

## Failure Modes
Stop or hand off when required evidence lists, source-map links, source locators, collection times, source owners, dates, commit references, or safe-use boundaries are missing. Preserve the missing label rather than guessing, and leave the evidence in a blocked review zone.

Block progression when evidence is stale without a refresh route, contradicted by a higher-authority source, restricted from citation, unsafe to inspect, dependent on inaccessible private material, generated without source evidence, or derived without traceable parent IDs. The blocked reason must remain visible for contradiction, negative-knowledge, synthesis, or human-review steps.

Use `stop_or_handoff` with exact IDs and next owner when the needed route is source-map repair, evidence collection, evidence ingestion, query refresh, human owner review, restricted-evidence handling, contradiction/gap classification, negative-knowledge capture, or durable-domain review. The output should name whether the next owner is an upstream research step, the human reviewer, a query/freshness route, or a later durable KB pipeline gate.

Forbidden actions are absolute: do not collect evidence, browse live sources, extract final claims, build the claim ledger, resolve contradictions, synthesize findings, write durable KB seeds, approve promotion, write canonical durable knowledge, mutate durable KB storage, update indexes or front doors, persist active pipeline state, edit registry/status/ledger files, execute researched external code, capture procedures, debug systems, deploy, or claim the Research Slice is complete.
