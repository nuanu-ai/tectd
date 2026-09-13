---
id: "slice-op-exec-risk-stop-condition-checker"
kind: "internal_step"
owner_family: "slice_variant"
pipeline_id: "slice.operational-execution"
step_id: "slice-op-exec-risk-stop-condition-checker"
entry_gate: false
public_trigger: false
native_skill_discovery: false
body_path: "capabilities/instructions/pipelines/slice-variants/operational-execution/slice-op-exec-risk-stop-condition-checker.step.md"
source_manifest: "capabilities/pipelines/slice-variants/operational-execution.pipeline.json"
legacy_skill_ref: "slice-op-exec-risk-stop-condition-checker"
architecture_refs:
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#operational-preparation-and-operational-execution-variants"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19"
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#operational-and-hybrid-slice-variants"
---

# Operational Execution Risk Stop Condition Checker

## Overview
This skill checks whether an operational execution slice is allowed to continue after preflight. Its core rule is zero unauthorized mutation: classify the declared operational stop conditions from existing evidence, then either pass to the next manifest step or stop for handoff.

## When to Use
Use when `authority-confirmation.md`, `current-state.md`, and `preflight.md` exist or their absence is itself the risk being evaluated, and the next question is whether final approval or action planning may proceed.

Do not use for preparation-only checklists, dry-run design, current-state capture, preflight execution, final approval, command execution, rollback, post-action validation, result writing, or procedure promotion.

## Source Contract
This is the Tect-owned body for `slice-op-exec-risk-stop-condition-checker` in `capabilities/pipelines/slice-variants/operational-execution.pipeline.json`.

Architecture grounding:
- `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#operational-preparation-and-operational-execution-variants`
- `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#operational-and-hybrid-slice-variants`
- `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6`
- `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19`

Manifest anchors: `pipeline.slice.operational-execution`; `step_graph.steps.slice-op-exec-risk-stop-condition-checker`; invoke ref `slice-op-exec-risk-stop-condition-checker`. The step produces `risk-stop-conditions.md`, gates on `stop_conditions_declared`, returns `ready_for_next_step` on pass, and returns `stop_or_handoff` on failure. No external skill body is referenced for this exact step.

## Operating Procedure
1. Confirm the slice is already in operational execution, not preparation, debug, or hybrid implementation, and that explicit execution authority was recorded before this check.
2. Read only the declared operational records: target and desired final state, authority confirmation, current state, preflight result, rollback or recovery posture, proof requirements, and any source operation plan or handoff that supplied stop conditions.
3. Convert each stop condition into a concrete rule with trigger signal, evidence source, consequence, owner, and required response. Reject vague rules such as "be careful" or "stop if something looks wrong" as blockers.
4. Classify every rule as `pass`, `stop`, or `block`: `pass` means evidence satisfies the rule; `stop` means a declared unsafe or do-not-continue condition is already true; `block` means evidence, authority, freshness, rollback posture, or proof requirements are missing or contradictory.
5. Check the required risk dimensions: blast radius, irreversible or data-affecting action, production or user impact, credential and secret exposure, rollback availability, recovery threshold, observation requirement, incident escalation, and proof needed before any terminal-status or final-result wording.
6. Preserve the mutation boundary. Do not run commands, change files, touch services, approve retries, execute rollback, widen authority, or infer permission from prior approval. If a fresh probe is needed, route back to preflight or handoff.
7. Write the decision shape for `risk-stop-conditions.md`: evaluated conditions, classification table, evidence references, unresolved blockers, exact do-not-continue rules, and the single routing decision.
8. Route to `ready_for_next_step` only when all required stop conditions are declared and no rule is `stop` or `block`. Route to `stop_or_handoff` when any stop condition fires, required evidence is absent, rollback authority is missing, risk exceeds the approved boundary, or incident scope takes over.

## Outputs
Produce only the risk-stop-condition decision for this manifest step. `risk-stop-conditions.md` must contain the condition table, pass/stop/block verdicts, evidence references, rollback or recovery threshold, unsafe-state rules, and route: `ready_for_next_step` or `stop_or_handoff`.

When blocked, include the missing evidence or authority, the actor who can unblock it, and the safest next step. Do not create an execution log, post-action proof, result, durable-domain update, command ledger, rollback record, or promotion artifact from this skill.

## Verification
Verify that the frontmatter trigger starts with "Use when", the seven required H2 sections are present, and the Source Contract names the architecture paths plus `capabilities/pipelines/slice-variants/operational-execution.pipeline.json`.

Verify content by checking that the procedure classifies `pass`, `stop`, and `block`; preserves zero unauthorized mutation; names `risk-stop-conditions.md`; and routes only to `ready_for_next_step` or `stop_or_handoff`. Run the body-quality validator, trigger fixture validator, JSON fixture parse, exact H2 scan, scoped `git diff --check`, and trailing whitespace scan for this skill and its two fixtures.

## Failure Modes
Stop or hand off when authority is ambiguous, current-state or preflight evidence is stale or missing, stop conditions are undeclared or vague, any declared unsafe state is true, rollback or recovery authority is missing, the requested action exceeds approved blast radius, secrets would be exposed, or live incident handling supersedes the generic operation. Keep the zero unauthorized mutation boundary intact and name the exact reason the operation cannot proceed.

If the user asks to continue despite a fired stop condition, keep the `stop_or_handoff` route and name the required approval, rollback owner, incident path, or follow-up slice. Never downgrade a stop or block condition to pass in order to keep the pipeline moving.
