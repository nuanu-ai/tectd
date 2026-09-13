---
id: "slice-op-risk-and-impact-modeler"
kind: "internal_step"
owner_family: "slice_variant"
pipeline_id: "slice.operational-preparation"
step_id: "slice-op-risk-and-impact-modeler"
entry_gate: false
public_trigger: false
native_skill_discovery: false
body_path: "capabilities/instructions/pipelines/slice-variants/operational-preparation/slice-op-risk-and-impact-modeler.step.md"
source_manifest: "capabilities/pipelines/slice-variants/operational-preparation.pipeline.json"
legacy_skill_ref: "slice-op-risk-and-impact-modeler"
architecture_refs:
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#operational-preparation-and-operational-execution-variants"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19"
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#operational-and-hybrid-slice-variants"
---

# Operational Preparation Risk And Impact Modeler

## Overview
This skill models risk and impact for a preparation-only operation package. Its core rule is: make consequence, reversibility, proof burden, and authority limits explicit before command planning, without executing or authorizing the operation.

## When to Use
Use this in `slice.operational-preparation` after operation intent, authority boundary, and current-state baseline are available or explicitly marked missing. Select it when the user wants a safe operation package, checklist, rollback plan, proof criteria, or handoff, and the operation is not authorized for execution yet.

Do not use it to run commands, approve a deploy, decide that risk is acceptable, or perform live validation. Route to operational execution when explicit execution authority exists, to hybrid when code or configuration changes are needed, and to debug when the root cause or target state is unknown enough that risk cannot be bounded.

## Source Contract
Ground this behavior in `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#operational-preparation-and-operational-execution-variants`, `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#operational-and-hybrid-slice-variants`, `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6`, and `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19`.

The owning manifest is `capabilities/pipelines/slice-variants/operational-preparation.pipeline.json`. The step anchor is `step_graph.steps.slice-op-risk-and-impact-modeler`; it invokes this `skill_body`, produces `risk-impact.md`, gates `risk_and_blast_radius_recorded`, ends in `risk_ready`, and fails with `block_unbounded_operation`. Atom anchors include `pipeline.slice.operational-preparation` and `pipeline.slice.operational_preparation.op.risk.and.impact.modeler`. No external skill body is referenced for this step.

## Operating Procedure
1. Confirm the preparation inputs: operation target, desired final state, non-goals, authority boundary, and current-state baseline. If any input is absent, mark whether risk can still be bounded from known facts or whether the package must block.
2. Define blast radius by naming every potentially affected environment, service, repo, branch, database, queue, cache, API, chain, tenant, customer segment, user workflow, credential, and dependent actor. Separate direct impact from plausible secondary impact.
3. Classify impact across production availability, user-visible behavior, data integrity, privacy or credential exposure, security posture, cost, compliance, support load, and team coordination. Use plain severity labels and cite the evidence or unknown behind each label.
4. Model reversibility. Identify irreversible actions, destructive writes, migrations, external notifications, cache invalidations, on-chain actions, third-party state changes, or operations where rollback only means forward recovery. State the rollback dependency owner and required preconditions.
5. Estimate downtime and degradation risk. Include expected interruption window, degraded-mode behavior, monitoring signals, and the point where a planned operation becomes an incident.
6. Define stop conditions before command planning: ambiguous target, stale baseline, unexpected diff, unhealthy dependency, missing credential owner, elevated error rate, backup unavailable, rollback gap, proof gap, or authority mismatch.
7. Declare escalation and authority needs. Name who must approve or perform the future action, which authority remains missing, and when the Slice must route to operational execution, hybrid implementation, debug, manual handoff, or `block_unbounded_operation`.
8. Set the proof burden for later steps. For each material risk, name the pre-action evidence, post-action proof, observation window, and handoff evidence needed before anyone may claim the operation completed.
9. End with a verdict: `risk_ready` only when blast radius, impact, reversibility, stop conditions, proof burden, and escalation needs are recorded well enough for preflight and command planning. Otherwise block with the exact missing bound.

## Outputs
Produce `risk-impact.md` with: target and final-state summary, impact matrix, blast-radius map, reversibility and rollback-dependency notes, data/user/production/security/credential sensitivity, downtime or degradation estimate, proof burden, stop conditions, escalation and authority needs, unresolved assumptions, and final `risk_and_blast_radius_recorded` verdict.

The output feeds `preflight-checks.md`, `operation-plan.md`, `rollback-plan.md`, `proof-contract.md`, and `handoff.md`. It must preserve `prepared_not_executed` truth and must not contain an action ledger, execution log, deploy approval, rollback execution, raw secret values, or a claim that the operation is safe to run.

## Verification
Validate the body with `node tools/validate-internal-skill-body-quality.mjs --skill slice-op-risk-and-impact-modeler` and trigger scenarios with `node tools/validate-internal-skill-trigger-fixtures.mjs --skill slice-op-risk-and-impact-modeler`.

For actual use, verify that `risk-impact.md` names a concrete target, records blast radius, covers reversibility, data/user/production/security impact, states rollback dependencies, includes stop conditions, assigns authority and escalation needs, and names the proof burden for later validation. Confirm the skill has exactly the seven required H2 sections, references the owning manifest and architecture HTML sources, and distinguishes preparation risk modeling from execution approval.

## Failure Modes
Block with `block_unbounded_operation` when the target, final state, production boundary, data sensitivity, current-state baseline, rollback path, stop condition, proof source, credential owner, or action authority is too vague to bound risk. Do not downscope unknown impact into a lower severity label just to keep planning moving.

Route instead of continuing when the user grants execution authority now, when the operation requires code or configuration changes, when root-cause evidence is missing, when a runbook/procedure capture path is the actual goal, or when another actor must provide approval or sensitive context. Keep the Slice in preparation-only state until those gaps are resolved.

If risk analysis reveals irreversible production impact, protected data exposure, unsafe rollback, cross-team dependency, high downtime, or credentials that cannot be handled safely, record the concern and force an explicit handoff or escalation before preflight or command planning. Never convert a risk model into permission to act.

Keep the final decision tied to preparation truth: the package can say risk is bounded enough for the next planning artifact, blocked, or handed off, but it cannot say the future action is approved, acceptable to run, or complete. The zero-execution boundary remains.
