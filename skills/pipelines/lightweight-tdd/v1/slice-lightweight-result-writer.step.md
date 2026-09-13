---
id: "slice-lightweight-result-writer"
kind: "internal_step"
owner_family: "slice_variant"
pipeline_id: "slice.lightweight-tdd-development"
step_id: "slice-lightweight-result-writer"
entry_gate: false
public_trigger: false
native_skill_discovery: false
body_path: "capabilities/instructions/pipelines/slice-variants/lightweight-tdd/slice-lightweight-result-writer.step.md"
source_manifest: "capabilities/pipelines/slice-variants/lightweight-tdd.pipeline.json"
legacy_skill_ref: "slice-lightweight-result-writer"
architecture_refs:
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#lightweight-development"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19"
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#lightweight-and-debug-slice-variants"
---

# Lightweight Result Writer

## Overview
This skill closes a Lightweight TDD Slice by writing `result.md` from the highest validated truth already proven by focused implementation, verification, and any deploy-impact or handoff notes. The core rule is lower ceremony, not lower proof: the result may be compact, but it cannot upgrade local proof into deploy, live, durable-domain, team, or production truth.

The skill is a result writer only. It reads source inputs, classifies proof, writes or blocks the result artifact, and routes the next owner. It does not run tests, patch code, deploy, invoke live-system commands, promote durable knowledge, repair maintenance state, mutate branches, or claim completion beyond the recorded proof.

## When to Use
Use this when the active Slice variant is `slice.lightweight-tdd-development`, the request was small and bounded, and the TDD/proof loop has reached the result boundary. Expected inputs include `slice.md`, `tdd-notes.md` or `test-plan.md`, `implementation-notes.md`, `verification.md`, and any deploy-impact, deferred, handoff, or evidence notes that affect the final truth.

Do not use this to capture intent, load context, shape the Slice contract, select a test target, run the RED/GREEN loop, write implementation notes, run verification, decide promotion, or perform maintenance. If deployment, live validation, operational execution, broad architecture ambiguity, or user/team handoff is required before the claim is true, route there before recording a final result.

Select this skill only when the remaining work is result recording from existing artifacts. If proof is missing, stale, contradictory, or owned by another actor, this skill records `block_missing_result` or a handoff route instead of trying to collect the proof itself.

## Source Contract
Grounding sources:
- `capabilities/pipelines/slice-variants/lightweight-tdd.pipeline.json` step `slice-lightweight-result-writer`
- `capabilities/pipelines/slice-variants/lightweight-tdd.pipeline.json#step_graph.steps.slice-lightweight-result-writer.invokes.slice-lightweight-result-writer`
- `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#lightweight-development`
- `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#lightweight-and-debug-slice-variants`
- `docs/architecture/master-plugin-target-architecture-part-2-spine-object-contracts.html`
- `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6`
- `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19`
- `docs/architecture/master-plugin-target-architecture-capability-atom-shard-map.html#pipeline.slice.lightweight_tdd.result.writer`

The manifest produces `result.md`, gates on `highest_truth_recorded`, fails with `block_missing_result`, and reaches `result_ready`. The related atom is `pipeline.slice.lightweight-tdd`, with the result-writer mapping anchored at the lightweight TDD manifest step.

Part 2 defines the proof contract: result is not implementation execution, but the highest validated truth the Slice reached, including environment tested, actor/authority, proof captured, blocked state, residual risk, and promotion target. Final map section `#s6` separates local proof, deployment proof, live proof, evidence proof, terminal states, and result/promotion ownership. Final map section `#s19` says lightweight changes have smaller artifact shape and lifecycle depth, not lower proof obligation.

## Operating Procedure
1. Confirm the result boundary. Read `slice.md`, `tdd-notes.md` or `test-plan.md`, `implementation-notes.md`, `verification.md`, deploy-impact notes, handoff notes, deferred notes, and evidence links when present. Record absent artifacts as absent, waived by contract, superseded, or blocking.
2. Build the source-input ledger. List request/scope, changed files or behavior, tests added or changed, commands already run, inspected artifacts, timestamps when available, actor/authority, target environment, and freshness state. Do not create new proof while doing this.
3. Classify `highest_validated_truth`. Use the strongest state the evidence supports: `completed_local_verified`, `completed_local_only`, `deploy_validation_required`, `live_validation_required`, `completed_user_handoff`, `completed_with_deferred_work`, `blocked_missing_proof`, `blocked_missing_authority`, `superseded_by_followup_slice`, or escalation to full, debug, hybrid, or operational flow.
4. Audit proof evidence claim by claim. Compare fixed, complete, safe, verified, no-deploy, no-risk, deployed, live, promoted, ready, and handed-off wording against RED/GREEN notes, focused and affected checks, local runtime evidence, reviewed files, deploy-impact decisions, authority records, and user/team validation. Downgrade or forbid every claim that exceeds proof freshness, target, actor, or environment.
5. Write the `result.md` body only when it can be honest. Include request and scope, source inputs, changed files or behavior, tests and commands already observed, proof evidence, proof class, `highest_validated_truth`, authority and freshness labels, deployment/live proof boundary, missing proof, residual risk, forbidden claims, deferred work, promotion candidate status, terminal state, and handoff or next action.
6. Set the manifest terminal state. Use `result_ready` only when `result.md` records `highest_truth_recorded` and preserves proof gaps, residual risks, forbidden claims, deployment/live limits, and next routing. Use `block_missing_result` when the artifact cannot be written truthfully; name the missing artifact, command output, verification scope, authority, freshness, deploy/live proof, or decision.
7. Route without performing routed work. Send reusable learning candidates to lightweight promotion only after result truth is written. Send paused, blocked, superseded, owner-return, or cleanup needs to maintenance/handoff. Send missing tests, verification, deployment, live validation, operational execution, full design, debug, or hybrid work to the owning step without running it.
8. Preserve authority boundaries. This skill may format `result.md` and final handoff text, but it does not authorize source mutation, test execution, deployment, live-system command, durable-domain write, promotion, branch operation, or maintenance repair.

## Outputs
Produce `result.md` for the selected Lightweight TDD Slice. The artifact must state:
- request and scope;
- source inputs read;
- changed files, changed behavior, tests, and commands already evidenced;
- proof evidence and freshness;
- proof class and `highest_validated_truth`;
- authority state and deployment/live proof boundary;
- missing proof, residual risk, and forbidden claims;
- deferred work, promotion candidate status, terminal state, and handoff or next action.

The only successful manifest terminal state for this step is `result_ready`. When the evidence cannot support truthful result text, emit `block_missing_result` with the missing proof, stale proof, target mismatch, missing authority, or unresolved deploy/live boundary instead of producing a completion-shaped result.

Preserve upstream truth inside the body when relevant: local verification passed, local-only result, deployment required, live proof missing, user/team handoff pending, deferred follow-up, superseded by follow-up Slice, or escalation required. Do not substitute those states for the manifest step state.

## Verification
Trigger verification requires positive scenarios where a Lightweight TDD Slice has completed focused implementation and verification and now needs final result recording, plus negative scenarios where test selection, TDD, verification, deploy/live proof, promotion, or handoff is still the next step.

Content verification requires the body to preserve the manifest path, architecture HTML sources, `result.md`, `highest_truth_recorded`, `block_missing_result`, and `result_ready`; to include concrete result-writing procedure; and to avoid wrapper sections, manifest dumps as behavior, execution authority, and any claim that local proof implies deploy or live truth.

Closure verification is a field-by-field proof audit. Every allowed final claim must cite an existing artifact, command result, inspection, user/team handoff, or proof note. Every missing proof must stay visible. Every forbidden claim remains forbidden until the owning pipeline records fresh proof. The deployment/live proof boundary is valid only when deploy-impact, deployment-validation, live-validation, or handoff evidence explicitly supports it.

Run:
- `node tools/validate-internal-skill-body-quality.mjs --skill slice-lightweight-result-writer`
- `node tools/validate-internal-skill-trigger-fixtures.mjs --skill slice-lightweight-result-writer`

## Failure Modes
Block with `block_missing_result` when required artifacts are absent without explanation, the RED/GREEN or acceptance proof was never observed, verification is stale, proof skips the changed behavior, implementation notes omit touched files or deviations, deploy impact is unresolved, authority is missing, or requested result text would upgrade local proof into deploy or live proof.

Route backward to intent, context, contract, test target, TDD, implementation notes, verification, or deploy-impact checks when more lightweight work is needed. Route sideways to full development, debug/root-cause, hybrid implementation/operation, operational execution, or user/team handoff when the lightweight Slice no longer owns the next action. Route forward to promotion only after `result.md` already records highest validated truth and any reusable learning candidate without writing to a durable domain.

If a user asks for a short final answer while the Slice evidence is incomplete, keep the result blocked and provide a handoff summary rather than softening the terminal state. If artifacts disagree, preserve the contradiction in `result.md` and route to the earliest step that can repair source truth. Treat that as zero-proof closure pressure, not permission to complete.

Forbidden actions are source mutation, patching, test execution, deployment, live-system command, durable-domain write, promotion, branch operation, maintenance repair, proof fabrication, and upgrading local proof to deployed or live truth. This skill can name those as required next actions, but it cannot perform or authorize them.
