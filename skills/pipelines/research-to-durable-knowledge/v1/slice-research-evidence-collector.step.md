---
id: "slice-research-evidence-collector"
kind: "internal_step"
owner_family: "slice_variant"
pipeline_id: "slice.research-to-durable-knowledge"
step_id: "slice-research-evidence-collector"
entry_gate: false
public_trigger: false
native_skill_discovery: false
body_path: "capabilities/instructions/pipelines/slice-variants/research-to-durable-knowledge/slice-research-evidence-collector.step.md"
source_manifest: "capabilities/pipelines/slice-variants/research-to-durable-knowledge.pipeline.json"
legacy_skill_ref: "slice-research-evidence-collector"
architecture_refs:
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#research-to-durable-kb-slice-variant"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19"
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#procedure-and-research-slice-variants"
---

# Slice Research Evidence Collector

## Overview
This skill collects raw research evidence for the Research To Durable Knowledge Slice. Its core rule is custody before conclusion: gather planned evidence into slice-local artifacts with citations, snapshot metadata, restrictions, and negative findings, but do not treat collected material as durable truth.

## When to Use
Use this after `source-map.md`, `evidence-plan.md`, `task-decomposition.md`, and any `subagent-assignments.md` are available for `slice.research-to-durable-knowledge`, and the next gate is `evidence_collected_or_gap_recorded`.

Use it when the Slice must gather local, external, runtime, session, transcript, log, repo, document, or web evidence for later ingestion, freshness labeling, claim extraction, contradiction review, synthesis, or durable-KB seed proposal.

Do not use it to frame research questions, design the source map, write the evidence plan, ingest the corpus log, label final provenance/freshness, extract claims, synthesize findings, promote knowledge, update indexes, mutate durable KB, execute researched code, run installers, deploy, operate live systems, or answer a simple current-state query.

## Source Contract
Ground this skill in `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#research-to-durable-kb-slice-variant`, `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#procedure-and-research-slice-variants`, `docs/architecture/master-plugin-target-architecture-part-6c-stateful-domain-pipelines.html#durable-kb-pipeline`, `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6`, and `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19`.

The owning manifest is `capabilities/pipelines/slice-variants/research-to-durable-knowledge.pipeline.json`, step `slice-research-evidence-collector`. The step is required, invokes `skill:slice-research-evidence-collector`, produces `evidence-corpus/index.md`, gates on `evidence_collected_or_gap_recorded`, reaches `ready_for_next_step`, and fails by `stop_or_handoff`.

Atom anchors are `pipeline.slice.research-to-durable-knowledge`, `pipeline.slice.research_to_durable_knowledge.evidence.collector`, and `capabilities/pipelines/slice-variants/research-to-durable-knowledge.pipeline.json#step_graph.steps.slice-research-evidence-collector`. External reference inputs are empty for this record.

Part 6C is a downstream durable-knowledge boundary: Durable KB turns promoted evidence into canonical knowledge only after authority, freshness, contradiction, provenance, and index/front-door gates. This collector may prepare slice-local evidence and gap records for that later route, but it must not create a `DomainPromotionRequest`, ingest durable source corpus, accept claims, write canonical KB objects, or update durable indexes.

## Operating Procedure
1. Verify the collection boundary. Require the research brief or questions, `source-map.md`, `evidence-plan.md`, source restrictions, freshness requirement, authority state, and any task or subagent assignment outputs. If the plan does not name source classes, collection methods, and stop conditions, route back to evidence planning.
2. Enumerate allowed source classes from the plan: local project docs, code, git history, generated indexes, prior research artifacts, existing KB or runbook pointers, session or memory excerpts allowed by privacy policy, logs, transcripts, screenshots, media, public web pages, external repos, vendor docs, standards, datasets, and read-only runtime snapshots. Use only source classes explicitly permitted by authority and policy.
3. Collect with the least side effect needed. Allowed methods are read-only file inspection, `rg` or equivalent local search, `git show`/`git log` inspection, safe document/transcript parsing, approved web or vendor-document lookup, downloaded or exported snapshots, screenshot or metadata capture, cached artifacts, user-provided files, and subagent-returned evidence packets. Do not run target code, installers, migrations, deploy commands, package scripts, destructive commands, or live-system probes unless a separate approved operations Slice owns that action.
4. For every evidence unit, capture citation data at collection time: source id, source class, path or URL when shareable, locator such as line, section, commit, timestamp, page, transcript offset, screenshot name, or log range, collection date, collector, access method, and a short relevance note tied to the research question.
5. Record provisional snapshot and freshness labels without promoting authority: `live_read`, `current_snapshot`, `historical_snapshot`, `source_declared_date`, `version_pinned`, `stale_risk`, `unknown_freshness`, or `needs_refresh`. Preserve source-declared dates separately from collection dates.
6. Enforce sensitivity and source restrictions. Redact or omit secrets, private personal data, credentials, proprietary payloads, unsafe exploit detail, restricted customer material, and source text that cannot be quoted safely. Keep a restricted-source placeholder with reason, owner, allowed use, and follow-up route.
7. Update slice-local source mapping as evidence is found. Add new source ids, aliases, discovered paths, skipped sources, access failures, and scope refinements as a `source-map.md` delta or sidecar note inside the Slice; do not update durable indexes or front doors.
8. Capture negative findings. For each planned source class or query that yields no usable evidence, record search terms, inspected locations, time bounds, access limits, and whether the result means `no_evidence_found`, `source_unavailable`, `restricted`, `too_stale`, `out_of_scope`, or `needs_human_access`.
9. Preserve subagent and user handoff boundaries. Treat subagent packets as evidence pointers that still need citation, source-class, snapshot, and allowed-use metadata. When a source requires credentials, private access, unsafe execution, or user-only judgment, record a `needs_human_access` or `needs_owner_review` gap with the exact question and next owner instead of expanding authority.
10. Build `evidence-corpus/index.md`. Group evidence by research question, task, source class, and confidence posture. Include raw-source pointers, citation table, snapshot metadata, restrictions, negative findings, source-map deltas, unresolved gaps, and the next owner for ingestion or handoff.
11. Close only when every required evidence-plan item is represented by collected evidence or an explicit gap. Use `ready_for_next_step` for a complete collected-or-gap-recorded corpus; otherwise use `stop_or_handoff` with named missing authority, source access, freshness, sensitivity, safety, subagent, or user blocker.

## Outputs
The manifest-required output is `evidence-corpus/index.md`. It must contain an evidence inventory, source ids, research-question linkage, citations and locators, collection dates, provisional snapshot/freshness labels, source restrictions, redactions or omissions, raw-source pointers, negative findings, source-map deltas, unresolved gaps, and handoff notes for ingestion.

The corpus handoff must name the next owner: normally `slice-research-evidence-ingester`, or `stop_or_handoff` to a user, source owner, or authorized subagent when access, sensitivity, freshness, or safety blocks collection.

Optional slice-local evidence artifacts may include `raw-sources/`, screenshots, transcript excerpts, exported logs, subagent-returned evidence packets, and source-map delta notes when the evidence plan allows them. These artifacts are collection custody records only; they are not durable KB entries, accepted claims, synthesis, promotion edges, index updates, `DomainPromotionRequest` packets, or canonical source truth.

## Verification
Verify the gate `evidence_collected_or_gap_recorded`: each planned source, source class, research question, and subagent/task assignment has either collected evidence with citation data or a negative/gap record explaining why evidence is missing, restricted, stale, unsafe, or out of scope.

Audit `evidence-corpus/index.md` for source ids, locators, collection timestamps, provisional freshness labels, access method, allowed-use notes, sensitivity handling, source-map deltas, and next owner. Confirm no raw source is described as durable current truth and no promoted claim appears before ingestion, provenance/freshness labeling, claim extraction, contradiction handling, and promotion gate review.

Confirm the Part 6C boundary is intact: no Durable KB corpus ingestion, canonical object write, claim acceptance, promotion result, query projection refresh, or index/front-door update is emitted from this step.

Validate the implementation with `node tools/validate-internal-skill-body-quality.mjs --skill slice-research-evidence-collector` and trigger coverage with `node tools/validate-internal-skill-trigger-fixtures.mjs --skill slice-research-evidence-collector`. Also parse the two owned fixture JSON files, check that the skill has exactly the seven Layer 6B H2 sections, run a trailing-whitespace scan, and run `git diff --check` scoped to this skill and its fixtures.

## Failure Modes
Stop or hand off when the evidence plan is missing, source classes are not permitted by policy, required sources cannot be accessed, network or live reads need approval, snapshots cannot be dated, sensitive material cannot be safely described, source restrictions block citation, or collection would require executing code, installers, deployments, migrations, or live-system operations outside the research collection authority.

Keep zero direct durable-write authority. Block progression when the corpus lacks citations, freshness context, negative findings for planned searches, source-map updates for discovered sources, or gap records for missing evidence. Do not let a partial corpus become synthesis, accepted claims, durable-KB seed content, promotion, or index/front-door updates.

Route elsewhere when the task is source-map design, evidence-plan design, ingestion/custody logging, provenance/freshness labeling, claim extraction, contradiction classification, negative-knowledge preservation, synthesis, durable object modeling, promotion approval, canonical KB mutation, procedure capture, debug/root-cause work, operational execution, deployment, or live validation.
