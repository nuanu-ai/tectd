---
id: "slice-procedure-source-context-loader"
kind: "internal_step"
owner_family: "slice_variant"
pipeline_id: "slice.custom-procedure-capture"
step_id: "slice-procedure-source-context-loader"
entry_gate: false
public_trigger: false
native_skill_discovery: false
body_path: "capabilities/instructions/pipelines/slice-variants/procedure-capture/slice-procedure-source-context-loader.step.md"
source_manifest: "capabilities/pipelines/slice-variants/procedure-capture.pipeline.json"
legacy_skill_ref: "slice-procedure-source-context-loader"
architecture_refs:
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#hybrid-implementation-operation-and-procedure-capture-variants"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19"
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#procedure-and-research-slice-variants"
---

# Procedure Source Context Loader

## Overview

This skill prepares the source context packet for `slice.custom-procedure-capture`. Its job is to identify and load the evidence basis for an unexpected reusable workflow before any source-event extraction, step normalization, runbook proposal, promotion gate, or skill-candidate routing begins.

Classification: `skill_body`. No external skill body is adapted by this step.

## When to Use

Use when the procedure-capture entry gate has already declared a proposal-only durable process candidate and the next question is what source material the later procedure steps may rely on.

Positive triggers:

- A source Slice, debug session, hybrid implementation, operation, or handoff contains a repeatable workflow candidate, but the relevant commands, logs, artifacts, and proof refs are scattered.
- A procedure-capture Slice needs to link the source work, session evidence, durable-domain context, and unavailable evidence before `slice-procedure-event-extractor` runs.
- The source event exists, but its relationship to an existing runbook, KB page, protocol note, operation record, or workspace artifact is not yet mapped.

Do not use when the work is still deciding whether to enter procedure capture, extracting what happened, normalizing steps, checking duplicates, scrubbing secrets, creating a proposal, approving promotion, or executing an existing runbook.

## Source Contract

Read the active procedure-capture packet, `slice.md` from the entry gate, available source Slice or session references, command history, logs, artifacts, proof refs, authority state, and secret-risk posture. Use read-side access only for durable-domain context such as runbooks, procedures, KB pages, protocol notes, operation records, or prior promotion results.

Grounding:

- `capabilities/pipelines/slice-variants/procedure-capture.pipeline.json`, step `slice-procedure-source-context-loader`, outputs `README.md` and `source-slice-link.md`, gate `source_context_loaded_or_declared_missing`, failure route `stop_or_handoff`.
- `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#hybrid-implementation-operation-and-procedure-capture-variants`.
- `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#procedure-and-research-slice-variants`.
- `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6`.
- `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19`.

This step may summarize and link source material. It must not alter durable knowledge, canonical runbooks, active skills, source repos, worktrees, deployments, package state, or live systems.

## Operating Procedure

1. Confirm the packet is already in `slice.custom-procedure-capture` and that `slice.md` names a procedure candidate, source event reference, authority posture, and secret-risk posture. If entry gating is missing, route back to `slice-procedure-capture-entry-gate`.
2. Build the source inventory. List the source Slice or session, parent Scope/Result if known, command history locations, logs, artifacts, screenshots, proof refs, decisions, and any unavailable source classes.
3. Map read-side durable context. Search or inspect only enough existing runbook, procedure, KB, protocol, operation, or prior result references to show which durable domains may matter. Do not decide duplicate status here; reserve that for `slice-procedure-existing-match-checker`.
4. Preserve evidence boundaries. Record exact paths, URLs, transcript/session identifiers, command snippets, timestamps, and proof pointers when available; when evidence is missing, say which source is missing and who or what could supply it.
5. Flag safety constraints without resolving them. Mark possible secrets, credentials, private environment details, destructive commands, deploy/live effects, or authority gaps for later authority-risk and secret-safety steps.
6. Create the context outputs. `README.md` states the context inventory, durable-domain read map, missing-source declarations, safety flags, and next step. `source-slice-link.md` links the procedure-capture Slice to the source Slice/session or declares why no stable source link exists.
7. Stop instead of inventing context. If the source event cannot be identified, evidence is hearsay-only, read access is unavailable, or sensitive material cannot be safely summarized, return `stop_or_handoff` with the exact missing or unsafe condition.

## Outputs

Primary outputs: `README.md` and `source-slice-link.md`.

`README.md` must include the selected variant, source event summary, evidence inventory, command/log/artifact references, durable-domain read-side references, unavailable source declarations, authority and secret-risk flags, and next step `slice-procedure-event-extractor`.

`source-slice-link.md` must include the source Slice/session identifier, parent object links when known, provenance of the link, and one of: linked, unstable-link, missing-source-link, or blocked-source-link.

Successful terminal state: `ready_for_next_step` with gate `source_context_loaded_or_declared_missing`.

## Verification

Before handoff, verify that the context packet distinguishes source loading from event extraction and proposal writing. The output should make later extraction possible without inventing steps, proof, or durable-domain conclusions.

Check that every cited source is either reachable in the current workspace/session context or explicitly marked missing. Check that durable-domain material is read-side context only, not a duplicate verdict or promotion decision. Check that secret, authority, deploy, and destructive-command risks are flagged for later steps rather than normalized away.

Fixture checks: `node tools/validate-internal-skill-body-quality.mjs --skill slice-procedure-source-context-loader` and `node tools/validate-internal-skill-trigger-fixtures.mjs --skill slice-procedure-source-context-loader`.

## Failure Modes

Return `stop_or_handoff` with `blocked_missing_source_event` when no source Slice, session event, command trail, artifact, log, or proof reference can anchor the procedure candidate.

Return `stop_or_handoff` with `blocked_source_access` when the needed context exists but current read authority, credentials, repo access, session access, or artifact availability is insufficient.

Return `stop_or_handoff` with `blocked_secret_risk` when the available material contains secrets, raw credentials, private environment details, or unsafe target identifiers that cannot be safely summarized.

Route away when the request is actually entry gating, event extraction, duplicate checking, proposal writing, promotion approval, skill authoring or installation, durable runbook mutation, or execution of an existing runbook.
