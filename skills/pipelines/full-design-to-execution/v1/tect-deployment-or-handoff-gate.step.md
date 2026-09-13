---
id: "tect-deployment-or-handoff-gate"
kind: "internal_step"
owner_family: "slice_variant"
pipeline_id: "slice.full-design-to-execution"
step_id: "tect-deployment-or-handoff-gate"
entry_gate: false
public_trigger: false
native_skill_discovery: false
body_path: "capabilities/instructions/pipelines/slice-variants/full-design-to-execution/tect-deployment-or-handoff-gate.step.md"
source_manifest: "capabilities/pipelines/slice-variants/full-design-to-execution.pipeline.json"
legacy_skill_ref: "tect-deployment-or-handoff-gate"
architecture_refs:
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#full-development-slice-variant"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19"
---

# Tect Deployment Or Handoff Gate

## Overview
This skill is the Full Design-To-Execution Slice approval gate between local verification and result closure. Its core rule is that local proof, deployment proof, live proof, and handoff proof are different truth levels, and the Slice may only advance along the route that the current authority and evidence actually support.

## When to Use
Use this after local verification and after the Slice's deployment/live-validation contract has been shaped for `slice.full-design-to-execution`. Select it when the Slice needs to choose one of these routes before closure: deployment validation by an authorized agent, live validation after deployment, user-restricted deploy handoff, team-managed deploy handoff, deployment not required, or blocked proof/authority.

Do not use it to collect missing local evidence, perform a deployment, run live validation, mutate source, write the final result, promote durable knowledge, or claim completion. Route evidence gathering to verification skills, actual live checks to the live-validation runner, result wording to the result writer, and operational side effects to an operational or hybrid Slice.

## Source Contract
Grounding sources are `capabilities/pipelines/slice-variants/full-design-to-execution.pipeline.json` step `slice-deployment-or-handoff-gate`, `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#full-development-slice-variant`, `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6`, and `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19`.

The manifest step is an approval gate with `authority_gate` and `proof_gate`. Its terminal-state candidates are `completed_deploy_verified`, `handoff_required`, and `blocked_missing_authority`; the broader Slice/Result pipeline may later classify local-only or live-verified closure, but only after the relevant writer or validator audits this gate verdict. This skill emits the route decision and required next proof only.

## Operating Procedure
1. Load the proof packet: Slice id, target environment, deployment contract, local verification summary, declared live-proof requirement, authority statement, rollback or recovery posture when required, and any existing deployment/live/handoff proof.
2. Normalize the proof ladder without upgrading it. `local_verified` means local commands or inspected artifacts passed. `deployment_verified` means there is a deploy trace tied to environment, actor, version or source ref, time, readiness, and rollback/recovery posture. `live_verified` means fresh user-visible/API/runtime/log/DB/chain confirmation after deployment. `handoff_recorded` means an accountable owner accepted a bounded action and return-proof contract.
3. Classify authority. Use `agent_authorized` only when the agent is explicitly allowed to perform the deployment path. Use `user_restricted_deploy` when the user must deploy or approve credentials/windows outside the agent. Use `team_restricted_deploy` when another team owns release or production authority. Use `missing_authority` when the actor, target, credentials, window, or approval basis is absent or ambiguous.
4. Choose the route from the matrix:
   - If deployment is not required and local proof is sufficient, route to result as local-only proof; do not call it deployed or live.
   - If deployment is required, authority is `agent_authorized`, and deployment proof is absent, route to deployment validation with the proof fields that the runner must return.
   - If deployment proof exists and live proof is required but absent, route to live validation with target, smoke surface, expected evidence, and stale-after condition.
   - If deployment proof exists and live proof is not required, set terminal-state candidate `completed_deploy_verified`.
   - If deployment or live proof depends on the user or a team, set terminal-state candidate `handoff_required` and require an owner, action, proof to return, resume trigger, and forbidden claims.
   - If authority, target identity, owner, deployment proof, live proof, or rollback/recovery posture is missing, set terminal-state candidate `blocked_missing_authority` or a named proof-gap block.
5. Emit the gate verdict. Include route, owner, authority state, proof level, missing proof, failure route, terminal-state candidate, output artifact target, next actor, resume trigger, stale-after condition, and forbidden claims.
6. Stop at the boundary. The next actor may be a deployment runner, live-validation runner, handoff builder, result writer, or follow-up Slice. This gate does not perform that actor's work.

## Outputs
The output is a route-only deployment-or-handoff gate verdict for the parent Slice. Its artifact shape is:

- `route`: `deployment_validation`, `live_validation`, `result_local_only`, `result_deploy_verified`, `user_handoff`, `team_handoff`, `blocked`, or `followup_slice_required`.
- `owner`: agent, user, named team, or unknown.
- `authority_state`: `agent_authorized`, `user_restricted_deploy`, `team_restricted_deploy`, or `missing_authority`.
- `highest_validated_truth`: `local_verified`, `deployment_verified`, `live_verified`, `handoff_recorded`, or `proof_gap`.
- `terminal_state_candidate`: `completed_deploy_verified`, `handoff_required`, `blocked_missing_authority`, or a downstream local/live state that must be confirmed by the result writer.
- `missing_proof`, `required_next_action`, `output_artifact_target`, `resume_trigger`, `stale_after`, `failure_route`, and `forbidden_claims`.

If handoff is selected, the verdict must identify the audience, action class, proof to collect, where returned proof belongs, and what wording remains forbidden until proof is audited. If blocked, it must name the missing authority or proof gap and the exact next question or follow-up Slice candidate.

## Verification
Verify the gate by checking that local proof, deployment proof, live proof, and handoff proof are represented as separate fields; that any user or team handoff has an explicit owner and return-proof contract; and that the selected route cannot exceed the highest validated truth. Negative scenarios must block claims that plan execution, passing local tests, a deployment request, fixture success, or stale memory proves deployment or live behavior. This skill's content is covered by `tools/validate-internal-skill-body-quality.mjs --skill tect-deployment-or-handoff-gate` and trigger selection by `tools/validate-internal-skill-trigger-fixtures.mjs --skill tect-deployment-or-handoff-gate`.

## Forbidden Actions
This gate does not deploy, redeploy, seed, rollback, live-validate, call live-system commands, mutate source, mutate worktrees or branches, persist active pipeline state, write `result.md`, write `promotion.md`, promote durable knowledge, or claim completion. It may only classify the current proof and route the Slice to the next authorized step.

## Failure Modes
Block instead of closing when local proof is missing, deployment authority is absent, user-restricted deploy has no accepting owner, team-managed release has no accountable team contact, the target environment is ambiguous, credentials or production window are unavailable, rollback or recovery posture is required but missing, deployment proof is stale or missing, live validation is required but not captured, or no proof owner can be named. Route to a follow-up Slice when the target, authority, or risk changes materially. Route to live validation when deployment proof exists but runtime proof is still needed. Route to result/promotion only after this gate's route and proof posture are explicit. Use zero-success wording for unresolved proof gaps.
