---
id: "slice-op-proof-contract-builder"
kind: "internal_step"
owner_family: "slice_variant"
pipeline_id: "slice.operational-preparation"
step_id: "slice-op-proof-contract-builder"
entry_gate: false
public_trigger: false
native_skill_discovery: false
body_path: "capabilities/instructions/pipelines/slice-variants/operational-preparation/slice-op-proof-contract-builder.step.md"
source_manifest: "capabilities/pipelines/slice-variants/operational-preparation.pipeline.json"
legacy_skill_ref: "slice-op-proof-contract-builder"
architecture_refs:
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#operational-and-hybrid-slice-variants"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19"
---

# Slice Op Proof Contract Builder

## Overview

This reference adapter skill for `superpowers:verification-before-completion` turns evidence-before-claim discipline into an operational-preparation proof contract. The core rule is: write `proof-contract.md` so a later authorized actor knows exactly what proof, claim, and verification evidence is required; do not run proof commands, run operation commands, or claim the operation succeeded.

## When to Use

Use this when a `slice.operational-preparation` package has enough inputs to define post-action proof: `operation-intent.md`, `authority-boundary.md`, `current-state.md`, `risk-impact.md`, `preflight-checks.md`, `operation-plan.md`, and `rollback-plan.md`. The trigger is strongest when the user wants a deploy, seed, redeploy, rollback, data repair, service restart, chain/API action, or infrastructure handoff with later proof criteria but no execution authority now.

Do not use this to create the command plan, run preflight, execute, deploy, write, delete, seed, migrate, restart, rollback, inspect live state after action, audit existing evidence, or write the final result. If authority is granted now, escalate to `slice.operational-execution`. If code/config work is needed, escalate to `slice.hybrid-implementation-operation`. If the target behavior is unexplained, route to debug/root-cause first.

## Source Contract

- Architecture: `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#operational-and-hybrid-slice-variants` and `#operational-preparation-and-operational-execution-variants` define operational preparation as exact command, rollback, proof, and handoff packaging without target mutation. `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6` separates local, deployment, live, evidence, and user proof. `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19` selects operational prep for ops targets with no execution.
- Manifest: `capabilities/pipelines/slice-variants/operational-preparation.pipeline.json` step `slice-op-proof-contract-builder` is a required validator step that produces `proof-contract.md`, gates on `post_action_proof_declared`, fails with `block_missing_proof_contract`, and reaches `proof_contract_ready`.
- Reference: `skills/references/superpowers/verification-before-completion/SKILL.md` is source material for proof-before-claim behavior only. Adapt its completion gate into a future-proof contract; do not import its runtime authority to run the verification now.

## Operating Procedure

1. Confirm the Slice is operational preparation and the authority boundary is prep/read/handoff only. If execution, deployment, live validation, rollback execution, or write authority is already requested, stop this skill and route to the correct execution or hybrid variant.
2. Load the prerequisite artifacts: operation target and desired final state from `operation-intent.md`, user approval and authority boundary from `authority-boundary.md`, baseline facts from `current-state.md`, risk and stop states from `risk-impact.md`, preflight expectations from `preflight-checks.md`, the command plan from `operation-plan.md`, and rollback path from `rollback-plan.md`. If any input is missing or stale enough to make proof design unsafe, return `block_missing_proof_contract`.
3. Enumerate every future claim that might be made after execution: prepared, command ran, deployed, seeded, migrated, rolled back, aborted safely, healthy, live, user-visible, completed, or partially blocked. For each claim, write the allowed claim only after named proof exists and the forbidden claim while proof is absent.
4. Convert each command-plan step into an exact future proof row. Include the exact future proof command or check, cwd, required environment or credential presence without secrets, expected output, expected exit code, timeout/window, source class, freshness window, evidence actor, user approval needed, evidence path, and stop state if the row fails or is unavailable.
5. Map desired final state to proof classes. Cover only applicable classes: logs, status endpoints, DB query, version or revision check, API smoke, UI screenshot, queue/job status, chain event or receipt, config state, service health, and user confirmation. Do not write vague checks such as "verify logs"; name the command, endpoint, query, dashboard, screenshot target, or manual confirmation owner.
6. Separate proof authority. Mark each row as local, deployment, live, rollback, abort, user-confirmed, or handoff-only. Deployment and live proof are not required by the prep manifest unless a later execution or hybrid variant is selected, but the contract must state what evidence would be required before those claims become allowed.
7. Define rollback proof and abort proof. Rollback proof must say what previous version/config/data/service state should be restored, how to verify that restoration, and when rollback is unsafe. Abort proof must say how a later actor proves no mutating command ran after a stop state, or how partial execution is contained and handed off.
8. Preserve sensitive boundaries. Do not copy secret values, tokens, private customer data, or unsafe environment details into the contract. Record credential requirements as presence checks, redacted evidence paths, or manual operator confirmation.
9. Write the verdict. Use manifest terminal state `proof_contract_ready` only when every planned operation claim has proof source, exact future check, expected output, source class, freshness, evidence actor, approval boundary, evidence path, forbidden claim, rollback or abort handling, and missing-proof stop state. Otherwise route through manifest failure `block_missing_proof_contract`; do not present that failure route as a successful terminal state.

## Outputs

Produce `proof-contract.md` in the selected Slice folder. It must contain:

- Prep-only boundary: no proof execution, no operation execution, no deployment or live command, no operation-completed claim, and terminal truth remains `prepared_not_executed`.
- Input summary: target state, desired final state, authority boundary, command plan source, rollback-plan source, risks, stop states, and user approval requirements.
- Claim matrix: allowed claim, required proof, forbidden claim until proof exists, source class, freshness, and terminal consequence.
- Future proof rows: exact future proof command or check, cwd/env requirements, expected output, expected exit code, timeout or observation window, evidence actor, evidence path, and stop/block state.
- Rollback proof and abort proof sections.
- Handoff routing for missing proof, restricted credentials, manual-only proof, deployment proof, live proof, user confirmation, and result proof auditing.
- Final verdict: terminal state `proof_contract_ready`, or failure route `block_missing_proof_contract` with exact missing proof.

This artifact is a contract for later verification. It is not an execution log, action ledger, post-action validation, authority confirmation, live proof packet, rollback execution record, deployment proof, result, promotion record, or runbook.

## Verification

Before handoff, inspect `proof-contract.md` against the manifest gate `post_action_proof_declared`. Every future claim must have a concrete proof row with source class, exact future check, expected output, exit code when command-based, freshness, actor, authority, evidence path, forbidden claim, stop state, and rollback or abort proof when relevant.

Also verify the negative boundary: no proof command was run, no target operation command was run, no deployment or live command was run, no secret value was copied, no operation-completed claim appears, and no deployment execution authorization or live-system command authorization is granted by this artifact. Deterministic fixture checks are:

- `node tools/validate-internal-skill-body-quality.mjs --skill slice-op-proof-contract-builder`
- `node tools/validate-internal-skill-trigger-fixtures.mjs --skill slice-op-proof-contract-builder`

## Failure Modes

Block with `block_missing_proof_contract` when the operation claim, target state, command-plan step, expected output, proof source, freshness window, evidence actor, user approval, evidence path, rollback proof, abort proof, or missing-proof stop state is unclear. Block or hand off when proof depends on credentials the agent may not inspect, production access the agent does not have, private data that needs redaction, a live incident, an unknown root cause, or an unsafe rollback path.

Route to `slice-op-command-plan-builder` when `operation-plan.md` is missing or too vague. Route to `slice-op-rollback-plan-builder` when rollback or recovery proof cannot be described. Escalate to `slice.operational-execution` when the next authorized step is to run the operation or collect post-action proof. Escalate to `slice.hybrid-implementation-operation` when implementation and live operation must be handled together. Route to result proof auditing only when evidence already exists and needs a verdict.

## Quick Reference

Input: prep-only operation package with target, authority, baseline, risks, preflight, command plan, and rollback plan. Output: `proof-contract.md`. Gate: `post_action_proof_declared`. Terminal state: `proof_contract_ready`. Failure route: `block_missing_proof_contract`. Truth: `prepared_not_executed`.
