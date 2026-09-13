---
id: "tect-live-validation-runner"
kind: "internal_step"
owner_family: "slice_variant"
pipeline_id: "slice.full-design-to-execution"
step_id: "tect-live-validation-runner"
entry_gate: false
public_trigger: false
native_skill_discovery: false
body_path: "capabilities/instructions/pipelines/slice-variants/full-design-to-execution/tect-live-validation-runner.step.md"
source_manifest: "capabilities/pipelines/slice-variants/full-design-to-execution.pipeline.json"
legacy_skill_ref: "tect-live-validation-runner"
architecture_refs:
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#full-development-slice-variant"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19"
---

# Tect Live Validation Runner

## Overview
This is the standalone live-validation proof auditor for a Full Design-To-Execution Slice. Core rule: local proof, deployment proof, and live proof are separate proof classes; completion claims need a fresh claim-to-proof matrix tied to the current target, source ref, authority, and observation window.

## When to Use
Use this after local verification and the deployment-or-handoff gate establish that live validation is required, deployment evidence exists, user/team deploy return evidence has arrived, or a result boundary is about to classify the Slice as live verified. Select it when evidence may include deploy traces, version/readiness data, logs, API behavior, database state, chain receipts, UI smoke checks, observation windows, or user confirmation.

Do not use it to decide whether deployment should happen, perform deployment, roll back a system, grant live-system access, collect broad local evidence, mutate source, write the final result, or promote durable knowledge. Route deployment choice to the deployment-or-handoff gate, rollback/recovery action to an authorized operational or hybrid route, local checks to verification, side effects to operational or hybrid variants, and closure prose to the result writer.

## Source Contract
Ground this skill in `capabilities/pipelines/slice-variants/full-design-to-execution.pipeline.json` capability-surface ref `tect-live-validation-runner` and step `slice-live-validation-runner`, with terminal states `completed_live_verified`, `blocked_missing_live_proof`, and `not_required`. The owning architecture anchors are `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#full-development-slice-variant`, `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6`, `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19`, `docs/architecture/master-plugin-target-architecture-validation-harness-evals.html`, and atom row `pipeline.slice.full_design_to_execution.live.validation.runner` in `docs/architecture/master-plugin-target-architecture-capability-atom-shard-map.html`.

Classification is `skill_body`. The target registry record has no external/custom skill body reference. This skill evaluates proof and routing only; it does not perform deployment, does not authorize live-system commands, and does not create deployment authority, rollback authority, workspace write authority, source mutation authority, active-state persistence, durable-domain promotion, or final Result / Promotion closure.

## Operating Procedure
1. Load the declared proof contract from the Slice context: required proof level, target environment, source ref or revision, deployment proof requirement, live check methods, freshness window, permission state, observation-window requirement, rollback/recovery expectation, and existing `verification.md`, `deployment-validation.md`, evidence packet, handoff, or proof verdict.
2. Require deployment evidence before live proof can pass. Accept direct deployment evidence or user/team deploy return evidence only when it names target environment, deployed source ref/version, actor, timestamp, readiness signal, and where the returned proof came from. If deployment is user/team managed and the return package is missing or ambiguous, choose handoff or blocked posture instead of treating local proof as deploy proof.
3. Build a claim-to-proof matrix before evaluating closure wording. Each row must name the requested claim, required proof class, supplied evidence, target identity, source ref, freshness basis, authority basis, gaps, and the maximum supported claim. Keep local proof class, deployment proof class, live proof class, rollback proof class, and user-confirmation proof class separate.
4. Decide whether live validation is required. Return `not_required` only when the variant contract explicitly says local or deployment proof is the completion boundary and no downstream claim uses live, production, user-visible, deployed-and-working, user-verified, rollback-safe, or equivalent wording.
5. Check permission before collecting or relying on evidence. Use only evidence already captured or checks permitted by the parent authority packet. If a credentialed probe, production observation, user confirmation, or chain/API/DB query is needed but not permitted, stop with a blocked or handoff posture instead of implying the agent can complete live proof.
6. Apply observation window rules when the proof contract requires monitoring. Record observation window start, observation window end, duration or event count, watched signals, expected stable state, stale-after condition, reset condition, negative signals, and whether the window completed without contradiction. An interrupted, wrong-target, pre-deploy, or stale window cannot support live proof.
7. Classify each proof item by source class, target, source ref, timestamp, freshness, method, observed result, and failure or negative signal. Keep deploy trace, readiness signal, live smoke, logs, API, DB, chain, UI, observation-window, and user-confirmation proof separate so one class cannot silently upgrade another.
8. Normalize evidence only after the checks above are complete. Deployment started is not deployed. Deployed is not live verified. Staging proof is not production proof. A screenshot, fixture pass, stale memory, generated projection, or historical log is not current live health unless the contract explicitly accepts it and freshness is current.
9. Select the terminal posture: `completed_live_verified` when live evidence covers the declared methods for the current target; `blocked_missing_live_proof` when proof, freshness, authority, target identity, deployment evidence, or observation is insufficient; or `not_required` when the proof contract does not require live validation.
10. Emit forbidden claim boundaries for downstream Result writing. Name any wording that must be downgraded, such as complete, deployed, live, production-ready, fixed, user-verified, rollback-safe, or promoted when the matching proof class is absent.
11. Route the next action without performing it: result writer when live proof is supported or not required, deployment-or-handoff gate when authority ownership is unclear, user/team handoff when another actor must return deployment or live evidence, rollback/recovery route when live evidence contradicts the desired claim and rollback authority exists, follow-up Slice when live validation reveals new scope, or blocked proof state when no accountable proof path exists.

## Outputs
Return a structured `live_validation_verdict` for the parent Slice with `terminal_posture`, target identity, source ref, deployment evidence package, claim-to-proof matrix, evidence classes checked, observation window status, freshness basis, authority basis, supported claim level, missing deployment proof, missing live proof, negative findings, rollback/recovery route, residual risk, forbidden upgraded claims, and next route. When blocked, name the exact missing proof or authority gap. When handoff is needed, name the owner, action, evidence to return, stale-after condition, and where the returned proof must be audited.

The verdict may feed `deployment-validation.md`, `verification.md`, `handoff.md`, `deferred.md`, `recovery-notes.md`, and `result.md`, but this skill does not finalize them. It preserves the result boundary by giving the result writer the highest validated truth instead of optimistic wording.

## Verification
Verify the body with `node tools/validate-internal-skill-body-quality.mjs --skill tect-live-validation-runner` and trigger coverage with `node tools/validate-internal-skill-trigger-fixtures.mjs --skill tect-live-validation-runner`.

For content review, confirm that the skill references the full-design manifest, Part 6B full-development architecture, final map `#s6` and `#s19`, the validation harness, and the live-validation atom row. Check that local, deployment, live, user/team deploy return evidence, claim-to-proof matrix, observation windows, user-confirmation, freshness, authority, missing-proof, not-required, handoff, rollback/recovery routing, and forbidden-claim behavior are explicit. Negative review must reject plan-executed-as-done, local-proof-as-live-proof, deploy-requested-as-deploy-verified, fixture-success-as-live-health, stale-memory-as-current-truth, deployment execution, rollback execution, source mutation, result writing, durable promotion, and live-system command authorization.

## Failure Modes
Block with `blocked_missing_live_proof` when deployment proof is absent, live evidence is stale, the target environment or source ref is ambiguous, the observation window is incomplete, the evidence comes from the wrong environment, or the requested claim exceeds the collected proof. Block or hand off when credentials, production access, user confirmation, rollback observation, or other live authority is missing.

Return `not_required` only from an explicit proof contract, never from convenience or lack of access. Route to follow-up Slice when live validation reveals a new defect, new target, changed requirement, or operational recovery path. Route to rollback/recovery only as a next route, never as an action, when live evidence contradicts the desired claim and the parent authority packet permits recovery planning. Route back to deployment-or-handoff when ownership is unresolved. Never allow `completed_live_verified` from local tests, CI, deploy start, generated projection, stale memory, or fixture results alone.

Use zero-live-proof wording when no accountable proof path exists. Do not deploy, do not rollback, do not mutate source, do not write result, do not promote, and do not claim completion without matching proof.
