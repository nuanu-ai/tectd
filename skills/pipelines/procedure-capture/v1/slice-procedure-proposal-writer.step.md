---
id: "slice-procedure-proposal-writer"
kind: "internal_step"
owner_family: "slice_variant"
pipeline_id: "slice.custom-procedure-capture"
step_id: "slice-procedure-proposal-writer"
entry_gate: false
public_trigger: false
native_skill_discovery: false
body_path: "capabilities/instructions/pipelines/slice-variants/procedure-capture/slice-procedure-proposal-writer.step.md"
source_manifest: "capabilities/pipelines/slice-variants/procedure-capture.pipeline.json"
legacy_skill_ref: "slice-procedure-proposal-writer"
architecture_refs:
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#hybrid-implementation-operation-and-procedure-capture-variants"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19"
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#procedure-and-research-slice-variants"
---

# Procedure Proposal Writer

## Overview

This skill writes the proposal packet for `slice.custom-procedure-capture` after upstream procedure-capture steps have converted a real source event into safe, normalized, proofed inputs. Its job is narrow: assemble a candidate durable artifact proposal for review. It does not normalize raw steps, validate the proposal, approve promotion, publish durable storage, execute commands, create a skill, or write final result truth.

## When to Use

Use this only when the active manifest step is `slice-procedure-proposal-writer` in `capabilities/pipelines/slice-variants/procedure-capture.pipeline.json` and all proposal inputs already exist:

- `source-event.md` and `captured-steps.md`
- `normalized-procedure.md`
- `existing-match-check.md`
- `authority-risk.md`
- `proof-contract.md`
- `secret-safety.md`
- `reuse-fit.md`
- target lane from `slice-procedure-durable-target-selector`
- validation notes from `slice-procedure-validation-runner`, when that step ran

Do not use this for raw session cleanup, procedure execution, existing runbook execution, research-to-KB synthesis, canonical runbook or KB mutation, promotion approval, skill authoring, index/front-door updates, or final result writing.

## Source Contract

Binding sources:

- `capabilities/pipelines/slice-variants/procedure-capture.pipeline.json`, step `slice-procedure-proposal-writer`, which produces `procedure-proposal.md`, `runbook-draft.md`, and `command-recipe.md`, gates on `proposal_written`, fails to `stop_or_handoff`, and advances only as `ready_for_next_step`.
- `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#hybrid-implementation-operation-and-procedure-capture-variants`, which requires procedure proposal before promotion and forbids silent durable mutation.
- `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#procedure-and-research-slice-variants`, which keeps proposal, evidence, promotion, and result truth distinct.
- `docs/architecture/master-plugin-target-architecture-part-6c-stateful-domain-pipelines.html`, which assigns canonical runbook, KB, index, freshness, and repair ownership to durable domain pipelines.
- `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6` and `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19`, which route unexpected custom operations to procedure capture with proposal/promotion readiness and keep promoted durable content outside Slice execution state.
- `docs/architecture/master-plugin-target-architecture-capability-atom-shard-map.html#pipeline.slice.procedure_capture.proposal.writer`, which maps this atom to "Write proposed durable artifact without changing canonical source truth yet."

## Operating Procedure

1. Confirm the active target is the procedure-capture Slice artifact workspace and the manifest step is `slice-procedure-proposal-writer`. If the caller asks for execution, promotion, canonical storage mutation, skill creation, or final result truth, block and route to the proper next workflow.
2. Load only upstream normalized/proofed inputs. Do not inspect raw command history to invent missing steps; if the normalized procedure or proof contract is insufficient, stop with the missing input named.
3. Check source basis and safety gates: source event link, existing-match verdict, selected target lane, authority/risk labels, secret-safety verdict, reuse-fit verdict, proof contract, and validation notes. Any contradiction, secret risk, duplicate conflict, unclear authority, or missing proof blocks `proposal_written`.
4. Choose the proposal stance: new procedure candidate, update-existing candidate, runbook draft candidate, command recipe candidate, proof-order template candidate, skill-candidate referral, no-promote proposal, or follow-up Slice proposal. The stance must stay candidate-only.
5. Write `procedure-proposal.md` with these required sections: title and candidate status; source basis; selected target lane; existing-match handling; reuse-fit rationale; intended operator and prerequisites; parameters and environment placeholders; normalized step summary; expected outputs; proof contract; authority and risk posture; secret-safety status; stop conditions; rollback or recovery notes when known; residual risks; rejected alternatives; next gate and owner.
6. Write `runbook-draft.md` only when the target lane is runbook-like or update-existing-runbook. Mark it as draft/proposal text, cite the existing-match decision, preserve eligibility, proof order, authority, freshness, and stop conditions, and avoid accepted, canonical, indexed, or published wording.
7. Write `command-recipe.md` only when the normalized procedure includes commands or manual action sequences. Use parameters for local values, include preflight checks, expected outputs, stop conditions, rollback or recovery notes, and future proof to collect. Do not include secrets, raw credentials, tokens, private hosts, or unsafe environment-specific details.
8. Set gate `proposal_written` only when the packet is internally consistent, secret-safe, candidate-only, non-duplicative or explicitly update-oriented, and ready for `slice-procedure-validation-runner` recheck or `slice-procedure-promotion-gate` review. Otherwise return `stop_or_handoff` with the exact blocker and next route.

## Outputs

Primary artifact: `procedure-proposal.md` in the active procedure-capture Slice artifact set. It must include candidate status, source basis, target lane, existing-match handling, normalized steps summary, proof contract, authority/risk posture, secret-safety verdict, future proof collection, stop conditions, residual risks, and next owner or gate.

Conditional artifacts: `runbook-draft.md` for runbook-like or update-existing lanes, and `command-recipe.md` for command or manual-action procedures. These are proposal artifacts, not durable runbook library entries, not executable approval, and not final result truth.

Allowed gate and terminal outcomes:

- `proposal_written` plus `ready_for_next_step` when the packet is complete.
- `stop_or_handoff` when proposal writing is blocked or routed elsewhere.

Next-step routing is to `slice-procedure-validation-runner` for completeness/safety recheck, `slice-procedure-promotion-gate` for approval review, `slice-procedure-skill-candidate-router` for mature skill-candidate referral, runbook-library promotion workflow for accepted runbook mutations, or `slice-procedure-result-writer` only after the downstream step has established terminal truth.

## Verification

Before advancing, verify every proposal claim traces to an upstream procedure-capture artifact. Confirm the packet references `capabilities/pipelines/slice-variants/procedure-capture.pipeline.json` and `slice-procedure-proposal-writer`, contains the required `procedure-proposal.md` sections, and carries candidate-only wording.

Check that no artifact claims durable publication, canonical status, promotion approval, future execution success, live proof, index/front-door update, or final result truth. Confirm any command recipe is parameterized, secret-safe, has stop conditions, and says what proof future executions must collect.

For source maintenance, run `node tools/validate-internal-skill-body-quality.mjs --skill slice-procedure-proposal-writer` and `node tools/validate-internal-skill-trigger-fixtures.mjs --skill slice-procedure-proposal-writer`.

## Failure Modes

Return `stop_or_handoff` when source event, normalized procedure, existing-match verdict, selected target lane, authority/risk labels, proof contract, secret-safety verdict, reuse-fit verdict, or validation notes are missing or contradictory. Block when the packet would duplicate existing durable material instead of proposing an update, contains secrets or unsafe local detail, lacks operator authority, lacks future proof, or depends on raw steps that were never normalized.

Route instead of drafting when the request belongs to operational execution, existing runbook execution, research-to-durable-KB, durable runbook or KB publication, promotion approval, skill authoring, index/front-door maintenance, or final result writing. If the candidate is too context-bound, unsafe, not repeatable, or proof cannot be stated, write no-promote or follow-up handoff language rather than a durable artifact draft.
