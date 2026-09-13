---
id: "slice-op-preflight-check-builder"
kind: "internal_step"
owner_family: "slice_variant"
pipeline_id: "slice.operational-preparation"
step_id: "slice-op-preflight-check-builder"
entry_gate: false
public_trigger: false
native_skill_discovery: false
body_path: "capabilities/instructions/pipelines/slice-variants/operational-preparation/slice-op-preflight-check-builder.step.md"
source_manifest: "capabilities/pipelines/slice-variants/operational-preparation.pipeline.json"
legacy_skill_ref: "slice-op-preflight-check-builder"
architecture_refs:
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#operational-preparation-and-operational-execution-variants"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19"
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#operational-and-hybrid-slice-variants"
---

# Operational Preparation Preflight Check Builder

## Overview
This skill builds the prep-only preflight checklist for an operational package. Its core rule is: define what must be true before a future operation can be run, but do not run checks, mutate the target, or claim preflight passed.

## When to Use
Use this in `slice.operational-preparation` after operation intent, authority boundary, current state, and risk impact are known enough to shape prerequisites. Select it when the user wants exact commands, checklist, rollback, proof criteria, or handoff while execution is not authorized yet.

Do not use it to run read-only or dry-run checks; route that to `slice-op-dry-run-or-readonly-validator`. Do not use it after explicit execution authority exists; route to `slice-op-exec-preflight-runner` for operational execution preflight. If code or configuration changes are required, route to the hybrid variant. If root cause is unknown, route to debug first.

## Source Contract
Ground this behavior in `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#operational-preparation-and-operational-execution-variants`, `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#operational-and-hybrid-slice-variants`, `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6`, and `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19`.

The owning manifest is `capabilities/pipelines/slice-variants/operational-preparation.pipeline.json`. The step anchor is `step_graph.steps.slice-op-preflight-check-builder`; it invokes this skill, produces `preflight-checks.md`, gates `preflight_checks_defined`, ends in `preflight_ready` or `blocked_missing_preflight`, and fails with `block_missing_preflight`. Atom anchors include `pipeline.slice.operational-preparation` and `pipeline.slice.operational_preparation.op.preflight.check.builder`. No external skill body is referenced for this step.

## Operating Procedure
1. Confirm the preparation inputs: operation target, desired final state, non-goals, authority boundary, current-state baseline, and risk-impact notes. If any are absent, name the missing input and block the preflight package.
2. Define target identity precisely enough for another actor to avoid the wrong target: environment, service, account, repo or worktree, host, database, chain, route, version, contract, or tenant as relevant.
3. Translate authority into check classes. Separate checks the agent may only define, checks that a user or team must perform manually, and optional read-only or dry-run checks that require a later validator step. Record authority gaps instead of assuming access.
4. Build checklist categories: target identity, current-state freshness, workspace or repo cleanliness, credential presence without secret disclosure, environment readiness, service or endpoint health, dependency availability, rollback prerequisites, risk relation, stop conditions, and evidence required before execution.
5. For every check, specify check ID, owner, authority source, command or tool shape if known, read-only or dry-run mode, expected evidence, pass/fail criteria, stale-after timestamp or event, failure signal, evidence path or paste target, and whether it is required or optional.
6. Relate each check to rollback and risk: what risk it reduces, what rollback prerequisite it protects, and whether failing it blocks execution, requires manual approval, or makes handoff more suitable than agent execution.
7. Mark any mutating check as execution-only and defer it to a later operational execution preflight. The preparation package may describe a command shape, but it must not run it or claim the output was observed.
8. Add safety gates before the command plan: stale current state, dirty target, missing credential owner, ambiguous production boundary, missing rollback owner, unavailable health signal, missing pass/fail criteria, or unbounded blast radius must block or force manual handoff.
9. End with a verdict. Use `preflight_ready` only when every required check has an owner, authority, expected evidence, pass/fail criteria, stale-after rule, stop condition, and handoff suitability. Use `blocked_missing_preflight` when required evidence, authority, target identity, or safe check design is missing.

## Outputs
Produce `preflight-checks.md` with target identity, preparation assumptions, required preflight checklist, optional read-only or dry-run checks, manual handoff checks, authority gaps, owners, required evidence, pass/fail criteria, stale-after rules, stop conditions, rollback/risk links, handoff suitability, and the final `preflight_checks_defined` verdict.

The output feeds later `operation-plan.md`, `rollback-plan.md`, `proof-contract.md`, and `handoff.md`. It must preserve `prepared_not_executed` truth and must not contain an action ledger, execution log, observed-check result, deploy proof, rollback proof, raw credentials, preflight-passed claim, or operation-completed claim.

## Verification
Validate the body with `node tools/validate-internal-skill-body-quality.mjs --skill slice-op-preflight-check-builder` and trigger scenarios with `node tools/validate-internal-skill-trigger-fixtures.mjs --skill slice-op-preflight-check-builder`.

For actual use, verify that `preflight-checks.md` names the target, separates prep-only from execution authority, includes owner, expected evidence, pass/fail criteria, stale-after, stop condition, rollback/risk relation, and handoff suitability for each required check, assigns manual handoff items, and blocks when prerequisites are missing.

Confirm the skill has exactly the seven required H2 sections, references the owning manifest and architecture HTML sources, distinguishes this prep checklist from `slice-op-exec-preflight-runner`, and contains no claim that any future preflight check has already passed.

## Failure Modes
Block with `blocked_missing_preflight` when the target is ambiguous, current-state evidence is stale, authority gaps prevent safe checks, required credentials or owners are unknown, health checks cannot be defined safely, rollback prerequisites are absent, or the evidence needed to judge a future operation is unclear.

Keep the Slice in a zero-execution state until the missing check, actor, authority, or evidence source is resolved by the right owner.

Route instead of continuing when the user grants execution authority now, when the next step is to run read-only or dry-run validation, when implementation is required, or when debug evidence is needed before an operation can be planned. Hand off when another actor must collect evidence or approve a target. Never turn a prepared checklist into proof that the operation ran or succeeded.

If the checklist exposes a production boundary, protected branch, credential-risk, irreversible action, or missing rollback owner that was not captured earlier, return to the authority or risk step before command planning. If all checks are optional and no required evidence is named, treat the package as incomplete rather than ready.
