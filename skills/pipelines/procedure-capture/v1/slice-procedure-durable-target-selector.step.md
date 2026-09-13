---
id: "slice-procedure-durable-target-selector"
kind: "internal_step"
owner_family: "slice_variant"
pipeline_id: "slice.custom-procedure-capture"
step_id: "slice-procedure-durable-target-selector"
entry_gate: false
public_trigger: false
native_skill_discovery: false
body_path: "capabilities/instructions/pipelines/slice-variants/procedure-capture/slice-procedure-durable-target-selector.step.md"
source_manifest: "capabilities/pipelines/slice-variants/procedure-capture.pipeline.json"
legacy_skill_ref: "slice-procedure-durable-target-selector"
architecture_refs:
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#hybrid-implementation-operation-and-procedure-capture-variants"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19"
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#procedure-and-research-slice-variants"
---

# Procedure Durable Target Selector

## Overview

This Tect-owned skill selects one candidate durable destination for a captured procedure. It is proposal-only routing inside `slice.custom-procedure-capture`: choose where the later proposal should point, record why, and preserve rejected alternatives.

It does not normalize steps, scrub secrets, approve promotion, write canonical durable KB or runbooks, create active skills, execute procedures, or write result truth.

## When to Use

Use this after `slice-procedure-existing-match-checker` has produced `existing-match-check.md` and before downstream `authority-risk.md`, `proof-contract.md`, `secret-safety.md`, and `reuse-fit.md` are written. The trigger is a procedure-capture candidate with a source event, captured steps, a normalized procedure shape, and a need to choose the candidate target lane for `procedure-proposal.md`.

Do not use it for existing runbook execution, direct durable writes, promotion acceptance, canonical runbook updates, research-to-KB synthesis, active skill authoring, maintenance repair, operational execution, deployment, or live-system commands.

## Source Contract

Grounding: `capabilities/pipelines/slice-variants/procedure-capture.pipeline.json` step `slice-procedure-durable-target-selector`. The step produces `procedure-proposal.md`, gates on `target_selected_as_candidate_only`, stops through `stop_or_handoff`, and advances through `ready_for_next_step`.

Architecture anchors: `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#hybrid-implementation-operation-and-procedure-capture-variants`, `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#procedure-and-research-slice-variants`, `docs/architecture/master-plugin-target-architecture-part-6c-stateful-domain-pipelines.html`, `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6`, `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19`, `docs/architecture/master-plugin-target-architecture-capability-atom-shard-map.html#pipeline.slice.procedure_capture.durable.target.selector`, and `docs/architecture/master-plugin-target-architecture-capability-atom-shard-map.html#pipeline.slice.procedure-capture`.

Required source inputs are `slice.md`, `source-slice-link.md` when present, `source-event.md`, `captured-steps.md`, `normalized-procedure.md`, `existing-match-check.md`, any source logs or artifacts already linked by the Slice, and any user-stated desired durable target. Later downstream checks may confirm, narrow, or block the selection; this skill must not pretend they have already passed.

## Operating Procedure

1. Confirm pre-selector inputs. Require `source-event.md`, `captured-steps.md`, `normalized-procedure.md`, and `existing-match-check.md`. If any is missing, or the source event is not a procedure-capture candidate, stop with `stop_or_handoff`.
2. Read the existing-match verdict first. If it says an existing runbook, procedure, KB page, or skill candidate should be reused or updated, select `update existing target` as the candidate route and block duplicate creation.
3. Classify the durable target shape. Use `procedure note` for lightweight captured procedure shape; `runbook`, `command recipe`, or `proof template` for exact repeatable operator guidance; `durable operations KB` for durable lessons or decision rules; `DevOps/infra knowledge`, `security knowledge`, `protocol knowledge`, `product research`, or `operations knowledge` for stateful domain facts; `skill candidate` for recurring agent behavior; `follow-up Slice` for unresolved implementation, research, validation, or cleanup; `no durable target` when the candidate is one-off, too context-bound, or not worth promotion.
4. Map stateful-domain targets to their owner pipeline without writing them: `runbook-library-pipeline`, `durable-kb-pipeline`, `devops-infra-pipeline`, `security-knowledge-pipeline`, `protocol-knowledge-pipeline`, `product-research-pipeline`, or `operations-knowledge-pipeline`.
5. Apply target-selection checks. The selected lane must match the normalized procedure substance, respect the existing-match verdict, name the future owner, preserve proof needs for downstream steps, and avoid claiming authority, safety, reuse, freshness, or promotion before the later gates run.
6. Choose a single primary candidate target. Put plausible secondary routes under rejected alternatives or deferred follow-up routes; do not create multiple simultaneous promotion targets.
7. Write only the target-selection block in `procedure-proposal.md`: selected candidate target, target owner pipeline or follow-up owner, target artifact shape, rationale from source inputs, rejected alternatives, existing-match effect, downstream checks required, gate `target_selected_as_candidate_only`, terminal state, and residual risks.
8. Route forward to `slice-procedure-authority-and-risk-classifier` when the candidate-only gate passes. Use `stop_or_handoff` when target ownership is unclear, source artifacts conflict, duplicate risk is unresolved, secret-bearing source material is visible, or the request asks for execution, promotion, canonical write, skill activation, or result truth.

## Outputs

Write or update only the selector-owned block of `procedure-proposal.md`. The block must contain:

- `skill`: `slice-procedure-durable-target-selector`
- `pipeline`: `slice.custom-procedure-capture`
- `selected_candidate_target`: one primary target
- `target_owner`: target pipeline, existing object, follow-up owner, or `none`
- `target_artifact_shape`: procedure note, runbook, command recipe, proof template, durable KB page, domain object, skill candidate, follow-up Slice, update existing target, or no durable target
- `source_inputs_used`: the input artifacts read
- `rationale`: evidence-linked reason for the target
- `rejected_alternatives`: rejected lanes and why
- `downstream_checks_required`: authority/risk, proof contract, secret safety, reuse fit, validation, proposal writer, skill-candidate routing, promotion gate, result writer, or maintenance handoff as applicable
- `gate`: `target_selected_as_candidate_only`
- `terminal_state`: `ready_for_next_step` or `stop_or_handoff`

This skill performs no durable knowledge write, no canonical runbook write, no promotion acceptance, no secret scrubbing, no active skill creation, no `result.md` write, and no source, workspace, deployment, or live-system mutation.

## Verification

Before advancing, verify that `procedure-proposal.md` references `capabilities/pipelines/slice-variants/procedure-capture.pipeline.json`, names `slice-procedure-durable-target-selector`, includes `target_selected_as_candidate_only`, names exactly one primary candidate target, records rejected alternatives, and avoids wording that says the target is accepted, promoted, canonical, indexed, executed, or written.

Check lane fidelity against pre-selector inputs: the existing-match verdict supports new versus update; the normalized procedure supports the selected artifact shape; the target owner belongs to a stateful domain, runbook library, skill-candidate route, follow-up Slice, or no-target route; and all risk, proof, secret-safety, and reuse claims are listed as downstream checks, not completed facts. For source maintenance, run `node tools/validate-internal-skill-body-quality.mjs --skill slice-procedure-durable-target-selector` and `node tools/validate-internal-skill-trigger-fixtures.mjs --skill slice-procedure-durable-target-selector`.

## Failure Modes

Use `stop_or_handoff` when prerequisite artifacts are absent, the normalized procedure is too vague to classify, the existing-match verdict conflicts with a new-target route, obvious secret-bearing source material remains, no owner pipeline fits, multiple primary targets cannot be reduced to one, or the user is asking for promotion rather than candidate routing.

Choose `no durable target` when the workflow was useful once but appears too project-specific, insufficiently evidenced, unsafe to generalize, or better retained only as local Slice history pending downstream reuse-fit review. Route to a follow-up Slice when the next move is research, implementation, validation, cleanup, domain-owner review, or capability-authoring review before any durable target can be responsibly proposed.

The only successful terminal state for this step is `ready_for_next_step`; the only blocked terminal route is `stop_or_handoff`. Do not perform promotion, durable mutation, active skill creation, or result truth writing in this skill; route those to later owners.
