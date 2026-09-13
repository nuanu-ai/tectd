---
id: "slice-op-exec-preflight-runner"
kind: "internal_step"
owner_family: "slice_variant"
pipeline_id: "slice.operational-execution"
step_id: "slice-op-exec-preflight-runner"
entry_gate: false
public_trigger: false
native_skill_discovery: false
body_path: "capabilities/instructions/pipelines/slice-variants/operational-execution/slice-op-exec-preflight-runner.step.md"
source_manifest: "capabilities/pipelines/slice-variants/operational-execution.pipeline.json"
legacy_skill_ref: "slice-op-exec-preflight-runner"
architecture_refs:
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#operational-preparation-and-operational-execution-variants"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19"
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#operational-and-hybrid-slice-variants"
---

# Pre-Mutation Preflight Runner

## Overview
This skill runs the pre-mutation gate for `slice.operational-execution`.
It proves that the approved target, authority, baseline, environment, and safe checks are still valid before any later mutating action is considered.
It may run only approved read-only or dry-run preflight checks and must never perform the operation itself.

## When to Use
Use this after `operation-intent.md`, `authority-confirmation.md`, and `current-state.md` exist for a bounded `slice.operational-execution` target.
Select it when the next decision is whether preconditions pass before risk-stop review, approval, command ledger construction, action running, rollback, or post-action proof.

Positive triggers:
- target identity, account, environment, repo, host, route, chain, database, or service must be rechecked against the approved target;
- access, credential presence, clean target posture, dependency state, service health, logs, config, read-only data, or dry-run output must be captured;
- freshness of `current-state.md` is uncertain and must be accepted, marked stale, or routed to refresh.

Do not use this for prep-only work with no agent authority, baseline creation, stop-condition modeling, final approval, action-ledger writing, mutating action running, rollback/recovery, post-action validation, result writing, or promotion.

## Source Contract
Classification: `skill_body`.

Ground this behavior in:
- `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#operational-preparation-and-operational-execution-variants`
- `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#operational-and-hybrid-slice-variants`
- `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6`
- `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19`
- `docs/architecture/master-plugin-target-architecture-capability-atom-shard-map.html#pipeline.slice.operational_execution.preflight.runner`

The owning manifest is `capabilities/pipelines/slice-variants/operational-execution.pipeline.json`.
The step anchor is `step_graph.steps.slice-op-exec-preflight-runner`.
It invokes this skill, produces `preflight.md`, gates `preflight_passed_or_blocked`, advances only to `ready_for_next_step`, and fails to `stop_or_handoff`.
The related atom anchors are `pipeline.slice.operational_execution.preflight.runner` and `pipeline.slice.operational-execution`.

## Operating Procedure
1. Load the source inputs: `operation-intent.md`, `authority-confirmation.md`, `current-state.md`, optional `source-operation-plan.md`, optional `dry-run.md`, optional `rollback.md`, known stop conditions, and any existing `preflight.md`.
2. Reconstruct target identity as a concrete environment target: service, account, cluster, repo/worktree, branch, host, route, database, chain, token, API, or other bounded surface.
3. Compare the reconstructed target with the authority record. If the target is missing, ambiguous, broadened, or different from approval, stop with `missing_authority` or `target_mismatch`.
4. Declare the approved preflight list before running anything. Each check must be explicitly read-only, dry-run, inspection-only, authentication-presence-only, health-probe-only, or another safe precondition.
5. Allowed checks include identity inspection, access probe, credential presence without secret value display, clean repo/worktree inspection, config read, version read, dependency availability, service health GET, read-only database query, chain/API read, log read, dry-run planning command, or no-op validation command.
6. Forbidden actions include writes, deploys, applies, seeds, migrations, deletes, restarts, transfers, credential rotations, branch changes, staging, committing, pushing, lock mutation, live traffic changes, rollback, or any command whose dry-run behavior is not documented.
7. Gate every check on authority. If a requested check is not approved as safe, record `skipped_not_authorized`; do not improvise a substitute that changes state.
8. Gate every check on freshness. Compare baseline timestamp, source class, expected version/config, current drift signals, incident state, pending deploys, branch state, and any user-provided newer evidence.
9. If the baseline is stale, missing, or contradictory, stop before running further checks unless the approved check exists solely to refresh read-only evidence.
10. Gate every check on proof. For each check, define expected output, acceptable exit statuses, evidence path, redaction rule, and stop condition before invocation.
11. Run only approved checks. Capture command or tool name, cwd or remote target, timestamp, expected output, observed output summary, exit status, raw output path when retained, redactions applied, and proof path.
12. Record command outputs under the Slice evidence location when available. Do not paste secrets, tokens, private keys, full credentials, or unrelated log dumps into `preflight.md`.
13. Classify each check as `passed`, `failed`, `stale`, `skipped_not_authorized`, or `blocked_needs_handoff`.
14. Evaluate results against declared stop conditions. Any failed, stale, unauthorized, ambiguous, or uninterpretable required check blocks progress.
15. Write or update `preflight.md` with the verdict and next route. Do not continue into risk-stop, approval, command ledger, mutating action, rollback, post-action validation, or result steps in the same skill invocation.

## Outputs
Produce or update `preflight.md`.

Minimum output shape:
- `target_identity`: exact environment target and approved boundary;
- `source_inputs`: files, timestamps, source class, and freshness basis used;
- `authority_scope`: safe checks allowed, missing authority, and checks rejected;
- `preflight_checks`: ordered check list with expected output, observed output summary, status, exit status, command outputs, evidence path, and proof path;
- `freshness_verdict`: fresh, stale baseline, conflicting baseline, or refresh required;
- `stop_conditions_seen`: none or named blockers;
- `final_verdict`: `preflight_passed_or_blocked`;
- `terminal_state`: `ready_for_next_step` or `stop_or_handoff`;
- `next_route`: `slice-op-exec-risk-stop-condition-checker`, `slice-op-exec-final-approval-gate`, `slice-op-exec-command-ledger-builder`, `slice-op-exec-authority-confirmation`, `slice-op-exec-maintenance-and-handoff`, or handoff to the named human/operator.

Use `ready_for_next_step` only when every required approved read-only or dry-run check passes, the baseline is fresh enough for the declared action, and proof is retained.
Use `stop_or_handoff` when authority is missing, baseline is stale, proof is absent, a check fails, a stop condition appears, or the next safe action belongs to another actor.

## Verification
Verify the body with:
- `node tools/validate-internal-skill-body-quality.mjs --skill slice-op-exec-preflight-runner`
- `node tools/validate-internal-skill-trigger-fixtures.mjs --skill slice-op-exec-preflight-runner`

Manual verification must confirm:
- exactly these seven H2 sections are present;
- the owning manifest, Part 6B, final map `#s6`, final map `#s19`, and atom-shard row are cited;
- the procedure is executable without another reference file;
- only approved read-only and dry-run checks are allowed;
- authority, freshness, and proof gates appear before any command;
- `preflight.md`, command outputs, evidence path, proof path, terminal states, and next routing are explicit;
- failed or stale checks remain blockers rather than completion proof.

## Failure Modes
Stop or hand off when authority is ambiguous, the check is not safely read-only or dry-run, the target identity does not match approval, the current-state baseline is stale, the environment target is unclear, service health or access fails, credential presence cannot be confirmed safely, dry-run output is missing, evidence conflicts, stop conditions trigger, proof cannot be retained, or the operator asks this skill to run the actual operation.

Route missing authority to `slice-op-exec-authority-confirmation` or `slice-op-exec-final-approval-gate`.
Route stale baseline to `slice-op-exec-current-state-baseliner`.
Route preflight risk blockers to `slice-op-exec-risk-stop-condition-checker`.
Route passed preflight toward approval or command ledger only through the Runtime-selected next step.
Route manual verification, external access, unclear ownership, or unsupported tooling to `slice-op-exec-maintenance-and-handoff` or a named human handoff.
Do not hide a blocker by downgrading a required check to optional after it fails.
