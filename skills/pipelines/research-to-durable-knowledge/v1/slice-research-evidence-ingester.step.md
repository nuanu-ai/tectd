---
id: "slice-research-evidence-ingester"
kind: "internal_step"
owner_family: "slice_variant"
pipeline_id: "slice.research-to-durable-knowledge"
step_id: "slice-research-evidence-ingester"
entry_gate: false
public_trigger: false
native_skill_discovery: false
body_path: "capabilities/instructions/pipelines/slice-variants/research-to-durable-knowledge/slice-research-evidence-ingester.step.md"
source_manifest: "capabilities/pipelines/slice-variants/research-to-durable-knowledge.pipeline.json"
legacy_skill_ref: "slice-research-evidence-ingester"
architecture_refs:
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#research-to-durable-kb-slice-variant"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19"
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#procedure-and-research-slice-variants"
---

# Slice Research Evidence Ingester

## Overview
This skill ingests already-collected research material into the Slice evidence corpus. Core rule: custody before interpretation. Every source pointer must become an evidence corpus row, duplicate alias, restricted item, rejected item, blocked item, or deferred gap before any downstream provenance labeling, claim extraction, synthesis, or durable promotion can use it.

## When to Use
Use this only inside `slice.research-to-durable-knowledge` after `slice-research-evidence-collector` or bounded research subagents have returned raw, local, external, runtime, session, transcript, screenshot, or generated-report evidence and the selected manifest gate is `evidence_ingested_with_custody`.

Use it when collected materials need stable evidence IDs, `source-map.md` links, locators, collection timestamps, source owners, access restrictions, allowed-use labels, safe excerpts or paraphrases, duplicate/stale/restricted handling, and intake provenance, freshness, and authority labels.

Do not use it to design the evidence plan, collect new evidence, browse live sources, dispatch subagents, execute researched code, extract final claims, synthesize final truth, resolve contradictions, model durable objects, approve promotion, promote durable KB, update front doors, mutate indexes, capture procedures, debug systems, or answer a direct query.

## Source Contract
Grounding sources are `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#research-to-durable-kb-slice-variant`, `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#procedure-and-research-slice-variants`, `docs/architecture/master-plugin-target-architecture-part-6c-stateful-domain-pipelines.html#durable-kb-pipeline`, `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6`, and `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19`.

The architecture text names the variant `slice.research-to-durable-kb`; the manifest and registry canonicalize this implementation as `slice.research-to-durable-knowledge`. Treat those as the same selected research Slice for this step, but do not rewrite architecture, registry, status, or ledger files from this skill.

The owning manifest is `capabilities/pipelines/slice-variants/research-to-durable-knowledge.pipeline.json`, step `slice-research-evidence-ingester`. The step is required, invokes `skill:slice-research-evidence-ingester`, produces `evidence-log.md`, gates on `evidence_ingested_with_custody`, reaches `ready_for_next_step`, and fails by `stop_or_handoff`.

The atom row is `pipeline.slice.research_to_durable_knowledge.evidence.ingester`: index evidence with source role, path, timestamp, owner, and allowed use. The Part 6C durable knowledge boundary says canonical durable knowledge belongs to the `durable-kb-pipeline` after authority, freshness, contradiction, provenance, canonical-write, and index/front-door gates. This skill does not own those gates.

## Operating Procedure
1. Confirm the Runtime-selected step is `slice-research-evidence-ingester` and the previous collection step produced source pointers. Required inputs are the research brief or questions, `source-map.md`, `evidence-plan.md`, raw source pointers or artifact paths, collector notes, and subagent reports when present. If the collected set or source-map boundary is missing, stop with `stop_or_handoff`.
2. Build an intake list before writing corpus rows. For each input source, record `source_map_id`, source class, source role, raw pointer or safe pointer, local artifact path when available, locator, collection timestamp, collector, source owner, acquisition method, access restriction, sensitivity, and relation to the research question.
3. Separate raw from derived material. Keep raw source pointers immutable and never overwrite raw artifacts. Store excerpts, paraphrases, generated summaries, observations, and normalized rows as derived custody records that point back to the raw source pointer and `source-map.md` entry.
4. Normalize evidence into stable Slice-scoped evidence IDs such as `E-001`. One source can produce multiple evidence units only when the observations are separable. Duplicate sources keep one canonical evidence ID plus duplicate aliases; do not delete duplicate, stale, negative, restricted, or contradictory evidence.
5. Attach intake labels to every evidence unit: `provenance_label`, `freshness_label`, `authority_label`, `sensitivity_label`, `allowed_use_label`, `retention_label`, and `citation_status`. Use explicit values such as `primary`, `secondary`, `generated_projection`, `user_supplied`, `runtime_snapshot`, `historical`, `current_at_collection`, `stale_risk`, `authoritative`, `advisory`, `restricted`, `unknown`, or `needs_labeling`. These are custody labels, not final claim adjudication.
6. Add safe excerpt or paraphrase material only within allowed-use and quote limits. Redact secrets, credentials, private identifiers, unsafe commands, malware/installers, and restricted content. Record the redaction reason and controlled source pointer when that pointer may be referenced.
7. Write `evidence-log.md` entries and `evidence-corpus/index.md` rows. Each row must include evidence ID, source-map link, source class and role, source path or safe pointer, locator, collection timestamp, collector or source owner, excerpt or paraphrase, raw-vs-derived status, access restriction, allowed use, intake labels, duplicate/stale/restricted status, rejection or deferral reason, and next owner.
8. Close the gate by count, not by impression. `collected pointers = accepted evidence IDs + duplicate aliases + rejected + restricted + blocked + deferred`. If the count balances and every accepted item has custody metadata, emit `evidence_ingested_with_custody` and route `ready_for_next_step` to `slice-research-provenance-and-freshness-labeler`. Otherwise stop or hand off with the exact missing rows.

## Outputs
Canonical output is `evidence-log.md`. It is the custody log for what was accepted, alias-linked, rejected, restricted, blocked, or deferred. It must be sufficient for a later agent to audit every collected pointer without re-collecting the source.

Secondary output is corpus-index content for `evidence-corpus/index.md`. Use a compact table or structured rows with: `evidence_id`, `source_map_id`, `source_role`, `pointer`, `locator`, `collected_at`, `source_owner`, `raw_or_derived`, `allowed_use_label`, `access_restriction`, `provenance_label`, `freshness_label`, `authority_label`, `status`, `next_owner`.

Valid next routing is `ready_for_next_step` to provenance/freshness labeling, then claim extraction, claim ledger, contradiction/gap handling, negative knowledge, synthesis, durable object modeling, seed proposal, promotion gate, promotion edge, index/front-door check, and result/handoff. This skill may name those next owners, but it must not do their work.

The output must contain no new evidence collection, no source execution, no final claim extraction, no claim synthesis, no contradiction resolution, no durable KB write, no promotion approval, no front-door/index mutation, no workspace mutation, and no whole-Slice completion claim.

## Verification
Verify source coverage by reconciling every collected source pointer to exactly one canonical evidence ID, duplicate alias, rejected item, restricted item, blocked item, or deferred gap. Verify row completeness by checking each accepted unit has a `source_map_id`, path or safe pointer, locator, collection timestamp, source owner or `unknown`, allowed-use label, excerpt or paraphrase, raw-vs-derived status, and provenance/freshness/authority labels.

Scan `evidence-log.md` and `evidence-corpus/index.md` for overreach: uncited claims, final truth language, synthesized conclusions, promotion decisions, canonical durable knowledge writes, front-door/index mutations, hidden restricted material, stale evidence treated as current truth, or raw sources copied into derived artifacts beyond allowed limits.

Run the targeted implementation validators after any body or fixture change:

```bash
node tools/validate-internal-skill-body-quality.mjs --skill slice-research-evidence-ingester
node tools/validate-internal-skill-trigger-fixtures.mjs --skill slice-research-evidence-ingester
```

## Failure Modes
Stop or hand off when required inputs are missing: source map, evidence plan, collected source pointers, locators, timestamps, access permission, allowed-use rule, source owner, custody metadata, or safe excerpt/paraphrase boundary. Do not reconstruct missing evidence from memory, chat summaries, generated projections, or assumptions.

Block progression when evidence contains secrets, credentials, private material, unsafe commands, malware/installers, restricted sources, unverifiable snapshots, broken links, or stale/currentness conflicts that cannot be safely labeled at intake. Preserve the blocked or restricted reason for later freshness, contradiction, negative-knowledge, or human-review steps.

Route elsewhere when the task is source mapping, evidence planning, evidence collection, final provenance/freshness labeling, claim extraction, claim-ledger building, contradiction/gap classification, synthesis, durable-object modeling, seed proposal, promotion gating, front-door/index updating, procedure capture, debug/root-cause investigation, direct query answering, or durable KB promotion.

Forbidden actions are absolute: do not collect new evidence, browse live sources, dispatch research, execute source material, overwrite raw sources, mutate source repos, persist active pipeline state, edit registry/status/ledger files, write canonical durable knowledge, update indexes, approve promotion, deploy, clean up, or claim the Research Slice is complete.
