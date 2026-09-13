---
id: "tect-validation-deployment-contract-shaper"
kind: "internal_step"
owner_family: "slice_variant"
pipeline_id: "slice.full-design-to-execution"
step_id: "tect-validation-deployment-contract-shaper"
entry_gate: false
public_trigger: false
native_skill_discovery: false
body_path: "capabilities/instructions/pipelines/slice-variants/full-design-to-execution/tect-validation-deployment-contract-shaper.step.md"
source_manifest: "capabilities/pipelines/slice-variants/full-design-to-execution.pipeline.json"
legacy_skill_ref: "tect-validation-deployment-contract-shaper"
architecture_refs:
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#full-development-slice-variant"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19"
---

# Tect Validation Deployment Contract Shaper

## Overview
This skill shapes the validation, deployment, live proof, handoff, and blocked-completion contract for a Full Design-To-Execution Slice. Its core rule is that completion must be defined before downstream closure language: local proof, integration proof, deploy proof, live proof, user/team handoff, and blocker states are separate truth levels with separate owners.

## When to Use
Use this inside `slice.full-design-to-execution` after the Slice has enough design, plan, execution, or verification posture to know whether deployment or live proof may affect completion. Select it when the parent Slice needs `deployment-validation.md` requirements, proof classes, authority constraints, forbidden claims, or terminal-state candidates before the deployment-or-handoff gate and result writer.

Do not use it to perform verification, deploy anything, check live systems, choose who deploys, write the final result, or promote durable knowledge. Route proof collection to verification or live-validation skills, route deployment choice to `tect-deployment-or-handoff-gate`, route closure wording to result/promotion skills, and route operational side effects to an operational or hybrid Slice variant.

## Source Contract
Grounding sources are `capabilities/pipelines/slice-variants/full-design-to-execution.pipeline.json` step `slice-validation-deployment-contract-shaper`, `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#full-development-slice-variant`, `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6`, `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19`, `docs/architecture/master-plugin-target-architecture-validation-harness-evals.html`, and atom row `pipeline.slice.full_design_to_execution.validation.deployment.contract.shaper` in `docs/architecture/master-plugin-target-architecture-capability-atom-shard-map.html`.

The manifest terminal states are `deployment_contract_ready`, `deployment_not_required`, and `blocked_missing_authority`. The registry record has no external/custom skill body source for this step; broad manifest references to Superpowers or custom spec skills belong to neighboring design, plan, verification, or handoff steps. This skill defines the contract only; it does not authorize pipeline execution, active-state persistence, workspace writes, source mutation, deployment, live-system commands, maintenance, promotion, or team merge behavior.

## Operating Procedure
1. Load the parent Slice posture: target object, selected full-development variant, Scope baseline, implementation and verification status, current proof artifacts, authority packet, target environment if known, and any existing `deployment-validation.md`, `verification.md`, `deferred.md`, or handoff notes.
2. Classify the completion dependency before any result wording is drafted. Use `local_required`, `integration_required`, `deployment_required`, `live_required`, `user_handoff_required`, `team_handoff_required`, `not_required`, or `blocked_unknown` as contract posture labels.
3. Separate proof classes. Local tests, integration checks, CI, deployment trace, readiness check, live API/UI/log/DB/chain behavior, observation window, and user confirmation must each have its own source, freshness rule, owner, and accepted claim level.
4. Build the contract field set. Include `contract_posture`, `target_object`, `target_environment`, `source_ref`, `required_proof_levels`, `proof_class_matrix`, `authority_owner`, `authority_state`, `handoff_owner`, `freshness_window`, `stale_after`, `required_artifact_writes`, `forbidden_claims`, `terminal_state_candidate`, `next_route`, `failure_route`, and `blocked_reason`.
5. Declare the authority boundary. Name who may perform deployment or live checks, what approval or credential is missing, whether the action is agent-managed, user-managed, team-managed, or blocked, and which claim words are forbidden until returned proof is audited.
6. Shape `deployment-validation.md` as a contract, not as proof. It should name required evidence, target identity, source ref, environment, expected verification method, stale-after condition, rollback or recovery expectation when relevant, handoff owner when external, terminal-state candidates, and where returned proof must be attached.
7. Set proof gates without running them. The contract must identify `local_gate`, `integration_gate`, `deployment_gate`, `live_gate`, and `handoff_gate` as required, not required, satisfied by existing proof, or blocked.
8. Set the downstream route without doing the downstream work. Route to the deployment-or-handoff gate when authority or deployment ownership must be chosen, to live validation when deployed/live proof is required, to result writing only when the highest allowed truth is clear, or to a blocked/deferred path when no accountable proof path exists.
9. Preserve false-completion guards. Explicitly state that plan execution, local proof, deployment request, fixture success, stale memory, generated projection, or old evidence cannot be upgraded into deploy or live completion.

## Outputs
Return a validation/deployment contract for the parent Slice. It must include contract posture, target object, target environment, source ref, required proof levels, proof class matrix, authority owner and state, handoff owner when external, missing approval or proof, freshness window, stale-after rule, required artifact writes, handoff conditions, forbidden claims until proven, terminal-state candidates, next route, failure route, and blocked reason when applicable.

The primary artifact write is `deployment-validation.md`. It records the contract fields above and references any supporting `verification.md`, `handoff.md`, `deferred.md`, `result.md`, or `promotion.md` paths that downstream steps must update. This skill may also return proposed write targets for those downstream artifacts, but it does not fill them with proof, result closure, or promotion content.

Expected terminal candidates are `deployment_contract_ready` when the next gate has a clear proof and authority contract, `deployment_not_required` when the Slice can close without deployment or live proof and forbidden live/deployed claims are absent, and `blocked_missing_authority` when required authority, target identity, proof owner, credentials, production window, or live-check permission is missing.

## Verification
Verify this skill by checking that the contract appears before result closure, references `capabilities/pipelines/slice-variants/full-design-to-execution.pipeline.json`, and preserves the architecture anchors for Part 6B full development, final map `#s6`, final map `#s19`, validation harness scenarios, and the deployment-contract atom row.

Content review must confirm that proof classes remain separate, `deployment-validation.md` is a contract rather than captured proof, authority and freshness are explicit, terminal candidates match the manifest, and downstream routing does not perform the downstream work. Negative review must reject any wording that treats plan execution as completion, local proof as live proof, deployment request as deployed proof, fixture success as real workspace health, stale memory as current truth, or this skill as permission to deploy, validate live, write the result, promote, or mutate source.

Fixture review must prove the contract fields and trigger boundaries, not merely the presence of architecture marker text.

## Failure Modes
Block with `blocked_missing_authority` when deployment or live proof is required but no accountable owner, target environment, approval, credential, observation window, or proof return path is available. Use `blocked_unknown` internally and request upstream clarification when the Slice target, selected variant, proof posture, or source ref is too ambiguous to shape a contract.

Return `deployment_not_required` only when the contract explicitly allows local or integration proof as the completion boundary and downstream wording avoids deployed, live, production-ready, user-verified, or equivalent claims. Hand off instead of closing when a user or team must perform deployment, return proof, or make an authority decision. Defer to a follow-up Slice when the target, risk, environment, or live-validation finding changes the work materially.

When no accountable path exists after these checks, use zero-proof wording: the Slice has a shaped contract gap, not a completed deployment or live validation state.
