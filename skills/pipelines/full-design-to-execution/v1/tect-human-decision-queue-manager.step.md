---
id: "tect-human-decision-queue-manager"
kind: "internal_step"
owner_family: "slice_variant"
pipeline_id: "slice.full-design-to-execution"
step_id: "tect-human-decision-queue-manager"
entry_gate: false
public_trigger: false
native_skill_discovery: false
body_path: "capabilities/instructions/pipelines/slice-variants/full-design-to-execution/tect-human-decision-queue-manager.step.md"
source_manifest: "capabilities/pipelines/slice-variants/full-design-to-execution.pipeline.json"
legacy_skill_ref: "tect-human-decision-queue-manager"
architecture_refs:
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#full-development-slice-variant"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#slice-full-design-to-execution"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#tect-work"
  - "docs/architecture/master-plugin-target-architecture-capability-atom-shard-map.html#pipeline.slice.full_design_to_execution.human.decision.queue.manager"
---

# Tect Human Decision Queue Manager

## Overview
Manage the human decision queue for the full design-to-execution Slice approval step. The core rule is approval/request-only: queue exact decisions, ask for exact human authority when needed, and block continuation until every blocking row is resolved by the right owner.

## When to Use
Use when the full design-to-execution Slice reaches `slice-human-decision-queue-manager`, when component interrogation, review, reconciliation, planning, deployment validation, result wording, or promotion pressure exposes a human-owned decision. Typical triggers include unresolved questions, waived requirements, user-owned choices, explicit approval needs, risk acceptance, contradiction resolution, or stale prior approvals after context drift.

Do not use this for component discovery, source edits, implementation, tests, deployment, live validation, Result closure, durable promotion, or final wording. Route those to the owning Slice, authority, Result, deployment, or promotion skill after this approval gate is clear.

## Scope Boundary
This skill owns the decision queue and approval request packet only. It does not approve on behalf of the user, infer consent from broad agreement, write source artifacts, continue the pipeline, mutate git or worktrees, run commands, deploy, validate live state, update durable knowledge, promote results, or claim work complete. A cleared queue permits only the manifest route `approved_to_continue`; the next step still performs its own authority, source, proof, and mutation checks.

## Source Contract
Grounding sources are `capabilities/pipelines/slice-variants/full-design-to-execution.pipeline.json`, required approval step `slice-human-decision-queue-manager`, invoking `tect-human-decision-queue-manager`, gated by `authority_gate`. Architecture grounding comes from `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html`, section 7 Full Development Slice Variant, where decisions include component interrogation, feed-forward decisions, and human decision queue; `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html` for `tect-work` and the full design-to-execution internal skillset; and `docs/architecture/master-plugin-target-architecture-capability-atom-shard-map.html` row `pipeline.slice.full_design_to_execution.human.decision.queue.manager`, which says to track human decisions, unresolved questions, waived requirements, and user-owned choices.

Manifest outputs affected by this queue are `design-spec.md`, `decisions/`, `implementation-ready-spec.md`, `implementation-plan.md`, `execution.md`, `verification.md`, `deployment-validation.md`, `result.md`, `promotion.md`, and `deferred.md`. This skill may propose queue rows and artifact update targets for those outputs; it is not the writer for them.

## Operating Procedure
1. Confirm the caller is the full design-to-execution Slice approval step or a direct retry of that step. Require Slice identity, parent context, current phase, affected artifacts, authority state, freshness state, and the proposed continuation boundary. If any is missing, stop with `handoff_required` or `blocked_missing_authority`.
2. Extract decision candidates from current Slice evidence: component decision files, decisions README status, design-spec gaps, cross-cutting review findings, reconciliation amendments, implementation-ready spec assumptions, plan risk, deployment-validation needs, result wording, promotion candidates, user waivers, and explicit questions.
3. Normalize each candidate into a queue row. Name the exact decision, why it blocks, affected artifacts, required owner, approval scope, risk or consequence, freshness basis, acceptable answers, default if no answer exists, and whether it blocks continuation or can be deferred.
4. Separate statuses: `queued_for_human`, `approved`, `rejected`, `waived_by_user`, `deferred`, `superseded`, `stale_context`, and `blocked_missing_authority`. Broad approval can resolve a row only when it names the same target, scope, risk, owner, and continuation boundary.
5. Apply context-drift survival before accepting any existing row. Recheck target identity, source freshness, authority owner, consequence, user instruction changes, manifest changes, and affected artifact set. Mark affected approvals `stale_context` when the answer no longer covers the current work.
6. Ask only exact questions. Each request must include the decision label, options, recommendation if any, risk, artifacts affected, what continuation it would allow, and what remains blocked. Do not ask optional preferences that are not blocking.
7. Route terminal state. Use `approved_to_continue` only when every blocking row is approved, waived, rejected with an alternate path, or explicitly deferred without invalidating the next step. Use `handoff_required` when a user, team, deploy owner, or future agent must answer. Use `blocked_missing_authority` when no safe approval request can be formed or required authority is absent.
8. Preserve false-completion guards. If decisions are unresolved, record missing proof or authority, prevent implementation/spec/result/promotion claims that rely on missing decisions, and route to handoff or follow-up Slice when the target changes materially.

## Queue Shape
Emit rows with these fields:

- `queue_id`, `decision_id`, `slice_id`, `source_step`, `source_artifact`, and `created_from_evidence`.
- `decision_prompt`, `decision_type`, `required_owner`, `authority_owner`, `approval_scope`, `risk_basis`, `freshness_basis`, and `blocking_scope`.
- `affected_artifacts`: any of `design-spec.md`, `decisions/`, `implementation-ready-spec.md`, `implementation-plan.md`, `execution.md`, `verification.md`, `deployment-validation.md`, `result.md`, `promotion.md`, or `deferred.md`.
- `status`: `queued_for_human`, `approved`, `rejected`, `waived_by_user`, `deferred`, `superseded`, `stale_context`, or `blocked_missing_authority`.
- `terminal_effect`: `approved_to_continue`, `handoff_required`, or `blocked_missing_authority`.
- `forbidden_claims`, `next_owner`, `retry_inputs`, and `context_drift_checks`.

## Outputs
Return a human decision queue packet with normalized rows, exact approval requests, blocking and nonblocking split, affected artifacts, stale-context rows, forbidden claims, terminal route, and retry inputs. The packet may propose where queue state should be reflected in `decisions/README.md`, decision files, implementation-ready spec notes, plan risks, verification gaps, deployment-validation handoff, result residual risk, promotion constraints, or deferred work.

The only terminal routes are `approved_to_continue`, `handoff_required`, and `blocked_missing_authority`. A route of `approved_to_continue` means the approval gate is clear, not that implementation, writing, deployment, validation, promotion, or completion has happened.

## Verification
Verify that every blocking decision has a source artifact or explicit user instruction, exact owner, exact approval scope, current freshness basis, and terminal effect. Check that no broad consent resolved a row outside its target, scope, risk, owner, or continuation boundary. Re-run context-drift checks before accepting any old approval after handoff, context overflow, target change, manifest change, consequence increase, or authority change.

Review the final packet for unsupported claims: queued is not approved, waived is not proof, deferred is not resolved, local approval is not deployment authority, and promotion candidate is not durable promotion. Confirm the body still names the manifest path, `slice-human-decision-queue-manager`, `authority_gate`, all three manifest terminal states, and at least one `docs/architecture/*.html` source.

## Failure Modes
Use `handoff_required` when the decision owner is the user, team, deploy owner, product owner, security owner, or future agent and an exact question can be asked. Use `blocked_missing_authority` when authority is absent, contradictory, stale, too broad, or owned elsewhere; when no exact approval question can be formed; or when the approval would conceal missing proof.

Invalidate or block when target identity changes, affected artifacts change, user instructions change, consequence increases, freshness expires, a decision was answered under an older scope, or a material target change requires follow-up Slice routing. If unresolved rows affect implementation-ready spec, implementation plan, execution, verification, deployment validation, result, promotion, or deferred work, prevent continuation and preserve the missing proof or authority.

## Forbidden Actions
Do not silently approve. Do not treat enthusiasm, acknowledgement, or partial answer as exact authority. Do not continue past required human decisions. Do not write lifecycle artifacts, edit source, run commands, mutate git or worktrees, deploy, validate live state, promote durable knowledge, update indexes, close Result state, or claim completion. Do not erase unresolved rows, hide stale approvals, or convert user-owned proof into agent-owned proof.
