---
id: "slice-procedure-authority-and-risk-classifier"
kind: "internal_step"
owner_family: "slice_variant"
pipeline_id: "slice.custom-procedure-capture"
step_id: "slice-procedure-authority-and-risk-classifier"
entry_gate: false
public_trigger: false
native_skill_discovery: false
body_path: "capabilities/instructions/pipelines/slice-variants/procedure-capture/slice-procedure-authority-and-risk-classifier.step.md"
source_manifest: "capabilities/pipelines/slice-variants/procedure-capture.pipeline.json"
legacy_skill_ref: "slice-procedure-authority-and-risk-classifier"
architecture_refs:
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#hybrid-implementation-operation-and-procedure-capture-variants"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19"
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#procedure-and-research-slice-variants"
---

# Procedure Authority And Risk Classifier

## Overview
This skill classifies authority and risk for a captured procedure candidate.
It is a proposal-only checkpoint: useful procedure evidence does not become a durable runbook, active skill, workspace mutation, deploy action, or promotion claim until authority and risk are explicit.
The skill records what later steps may consider; it does not execute, normalize, promote, scrub secrets, or write durable procedure truth.

## When to Use
Use this inside `slice.custom-procedure-capture` after these upstream inputs exist or are explicitly declared missing:

- `source-event.md` names the event or session that produced the candidate.
- `captured-steps.md` or `normalized-procedure.md` gives reusable candidate steps.
- `existing-match-check.md` says whether an existing runbook/procedure should be updated instead of duplicated.
- A candidate destination or durable target has been selected as proposal-only.

Select this skill when the candidate includes commands, environment assumptions, proof steps, handoff instructions, write locations, deploy actions, credentials, user-owned decisions, or promotion intent that needs an authority boundary.

Do not use it to execute an operation, follow an existing runbook, approve promotion, scrub secrets, validate reuse fit, normalize steps, or decide final procedure acceptance.

## Source Contract
Grounding sources are `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#hybrid-implementation-operation-and-procedure-capture-variants`, `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#procedure-and-research-slice-variants`, `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6`, and `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19`.

The owning manifest is `capabilities/pipelines/slice-variants/procedure-capture.pipeline.json`, pipeline `slice.custom-procedure-capture`, step `slice-procedure-authority-and-risk-classifier`.
The manifest makes this step required, produces `authority-risk.md`, records gate `authority_and_risk_recorded`, and exits as `ready_for_next_step` or `stop_or_handoff`.
The variant is proposal-only: no durable runbook write, active skill creation, workspace mutation, deployment, live-system command, package update, or silent promotion is authorized here.

## Operating Procedure
1. Confirm source inputs. Check for source event, captured steps, normalized procedure shape, existing-match result, and candidate target. If an input is missing, record the missing input and stop or hand off instead of guessing.
2. Inventory every action. Extract actions from the candidate and label each authority verb: read, inspect, propose, write, execute, deploy, promote, install, migrate, access secret material, or ask a human.
3. Split mixed actions. A step that starts as read-only but later writes, executes, deploys, promotes, or touches credentials must become separate rows with separate authority labels.
4. Classify risk per action. Record target environment, live or production consequence, reversibility, data-loss risk, security exposure, credential handling, privacy sensitivity, external dependency, team or customer impact, duplicate-runbook risk, and evidence freshness.
5. Decide the allowed boundary. For each action, choose one boundary: allowed inside the proposal, allowed only as quoted historical evidence, requires explicit user approval, must be performed by the user or team, must move to operational preparation or execution, must move to hybrid, must move to research, or must be blocked.
6. Declare allowed locations. Proposal artifacts may describe the candidate and proof needs. Durable knowledge, runbook libraries, command recipes, plugin skills, package files, workspace control-plane files, source repos, worktrees, and live systems are only targets for later authority-gated promotion or a different selected pipeline.
7. Add next safety gates. Mark whether the next step needs a proof contract, secret safety scrub, reuse fit evaluation, promotion approval, user-owned decision, or update to an existing artifact instead of a new artifact.
8. Set the step result. Use `ready_for_next_step` only when the classification names actions, risks, approvals, prohibited moves, allowed proposal locations, and next gates. Use `stop_or_handoff` when authority is missing, risk is unacceptable, the candidate belongs to another variant, or durable mutation is being requested too early.

## Outputs
Create or update `authority-risk.md` for the active procedure-capture proposal.
The record should include:

- source event reference and candidate durable target;
- action-by-action authority table;
- risk labels and approval requirements;
- allowed proposal locations;
- prohibited durable or live-side effects;
- required next gates;
- routing notes when approval, user execution, or variant routing is needed;
- terminal decision for this step.

Do not create or update canonical runbooks, durable KB pages, command recipes, plugin skills, package source, workspace control-plane truth, child source repos, worktrees, or live systems.

## Terminal States
Use `ready_for_next_step` only when `authority-risk.md` satisfies the manifest gate `authority_and_risk_recorded` and gives enough detail for proof-contract, secret-safety, reuse-fit, validation, proposal, and promotion-gate steps to continue safely.

Use `stop_or_handoff` when the candidate lacks authority, belongs to another Slice variant, requires human execution, contains unacceptable risk, depends on missing evidence, or asks for immediate durable mutation.

## Routing
Route existing-runbook execution to operational execution.
Route operation planning without execution to operational preparation.
Route code/config work plus deploy, seed, migration, rollout, or live validation to hybrid implementation plus operation.
Route source-only evidence gathering, claim extraction, or durable KB synthesis to research-to-durable-KB.
Route durable artifact mutation to the later promotion gate only after proof, secret safety, reuse fit, validation, and explicit authority are recorded.
Route mature repeatable procedure-to-skill ideas to skill-candidate routing only after this step has kept them inactive and proposal-only.

## Verification
Verify the record against the manifest and architecture before advancing.
Check that `authority-risk.md`:

- references `capabilities/pipelines/slice-variants/procedure-capture.pipeline.json`;
- includes the `authority_and_risk_recorded` gate;
- covers read, write, execute, deploy, promote, credential, privacy, and allowed-location risk where present;
- names required approval requirements;
- does not claim promotion, execution, durable mutation, active skill creation, live proof, or package/workspace/source mutation.

Trigger coverage must include positive procedure-capture classification scenarios and a non-trigger for existing runbook or operational execution.
The Layer 6B checks are `node tools/validate-internal-skill-body-quality.mjs --skill slice-procedure-authority-and-risk-classifier` and `node tools/validate-internal-skill-trigger-fixtures.mjs --skill slice-procedure-authority-and-risk-classifier`.

## Failure Modes
Stop or hand off when the source event is missing, captured steps are too vague, the existing-match check is absent, or the candidate includes secrets that need scrubbing before classification can continue.

Stop when authority for write, execute, deploy, promote, install, migrate, credential, or live-system actions is absent.
Stop when the action is irreversible, production-affecting, privacy-sensitive, duplicate-prone, or dependent on stale evidence and no explicit approval path exists.
Stop when the user is asking for immediate durable mutation from this step.

If risk is classifiable but not solvable here, write the blocker or handoff in `authority-risk.md` and route through the correct later gate.
