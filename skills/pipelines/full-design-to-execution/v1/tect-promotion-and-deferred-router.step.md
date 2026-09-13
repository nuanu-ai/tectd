---
id: "tect-promotion-and-deferred-router"
kind: "internal_step"
owner_family: "slice_variant"
pipeline_id: "slice.full-design-to-execution"
step_id: "tect-promotion-and-deferred-router"
entry_gate: false
public_trigger: false
native_skill_discovery: false
body_path: "capabilities/instructions/pipelines/slice-variants/full-design-to-execution/tect-promotion-and-deferred-router.step.md"
source_manifest: "capabilities/pipelines/slice-variants/full-design-to-execution.pipeline.json"
legacy_skill_ref: "tect-promotion-and-deferred-router"
architecture_refs:
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#full-development-slice-variant"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19"
---

# Tect Promotion And Deferred Router

## Overview
This skill is the Slice-local router for promotion candidates, no-promotion decisions, and deferred follow-up after `result.md` has established result truth. Its core rule is proof-bound classification only: record or propose routing artifacts, but never promote directly, mutate durable-domain storage, rewrite result truth, execute remaining work, or hide cleanup and follow-up obligations.

## When to Use
Use this at the `slice-promotion-and-deferred-router` step of `slice.full-design-to-execution` only after `result.md` exists and names the highest validated truth, proof state, residual risks, forbidden claims, and current terminal-state candidate. Select it when the completed Slice exposes durable learning candidates, repeated procedures, runbook candidates, domain knowledge, rejected ideas, no-promotion decisions, cleanup obligations, proof gaps, skill/rule candidates, user-owned handoffs, or follow-up Slice work that must be made visible before maintenance and handoff.

Do not use it before result truth exists, while proof is still being audited, or when the next action is to verify, deploy, write `result.md`, decide whether work is complete, clean a worktree, run maintenance, author a runbook, update canonical KB, repair indexes, or validate live behavior. Route those actions to the owning result, verification, deployment/live-validation, maintenance, domain, handoff, or git/worktree step.

## Source Contract
Grounding sources are `capabilities/pipelines/slice-variants/full-design-to-execution.pipeline.json` step `slice-promotion-and-deferred-router`, `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#full-development-slice-variant`, `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6`, `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19`, `docs/architecture/master-plugin-target-architecture-part-6c-stateful-domain-pipelines.html`, and atom `pipeline.slice.full_design_to_execution.promotion.and.deferred.router` in `docs/architecture/master-plugin-target-architecture-capability-atom-shard-map.html`.

The manifest invokes this skill as a service, produces Slice-local `promotion.md` and `deferred.md`, and permits only `promotion_candidate_recorded`, `no_promotion`, or `deferred_to_follow_up`. The whole-plugin map says Result/Promotion records highest validated truth, proof level, residual risk, deferred items, promotion target, cleanup obligations, and durable-domain handoff. Part 6C says a Slice or Result may request promotion, but `domain_entry_gate`, `promotion_request_loader`, authority/freshness assessment, `canonical_write_gate`, provenance/index work, and `promotion_result_writer` belong to the selected durable-domain pipeline.

## Operating Procedure
1. Confirm the precondition: parent Slice identity, selected variant contract, and `result.md` are present. `result.md` must name the highest validated truth, proof state, terminal-state candidate, residual risk, forbidden claims, and any missing proof. If any of these are absent, stop and route back to result writing or proof auditing.
2. Load only evidence-bearing Slice artifacts: `result.md`, `verification.md`, `deployment-validation.md` or `live-validation.md` when present, `execution.md`, `implementation-plan.md`, `implementation-ready-spec.md`, `design-spec.md`, `decisions/`, existing `promotion.md`, and existing `deferred.md`. Treat `result.md` as the highest authority for truth and do not rewrite it; contradictions route back to the Result/Promotion proof path.
3. Build a residue inventory from artifacts, not memory. Classify each item as `promotion_candidate`, `no_promotion`, or `deferred_follow_up`. Include reusable lessons, domain knowledge, runbook/procedure candidates, DevOps or operations facts, security/protocol/product research findings, rejected ideas, cleanup obligations, proof gaps, blocked work, user-owned handoffs, and skill/rule candidates.
4. For every row, record source artifact, exact claim or work item, proof level, proof state, freshness, authority, sensitivity, target owner, and first next proof. Preserve stale, contradictory, waived, unsupported, low-value, or intentionally rejected items instead of dropping them.
5. Route promotion candidates without promoting. A durable candidate needs a proof basis, target owner, and target domain such as durable KB, runbook library, DevOps infra, security knowledge, protocol knowledge, product research, operations knowledge, or skill authoring. The output is a promotion request packet for the domain entry gate and promotion request loader; canonical writes must wait for authority/freshness assessment, the canonical write gate, provenance/index work, and domain promotion result.
6. Route no-promotion decisions when nothing is worth promoting, a rejected idea should not resurface, evidence is insufficient, the item is stale or contradictory, or the value is too local. Record source, reason, proof limit, and reopen condition, then use terminal state `no_promotion`.
7. Route deferred follow-up when proof, cleanup, live validation, user/team action, domain intake, or component work remains outside this Slice. Every deferred item needs an owner, first next action, required proof, target artifact or follow-up Slice, freshness limit, and visible blocker; use terminal state `deferred_to_follow_up`.
8. Select the route state. Use `promotion_candidate_recorded` only for at least one proof-backed promotion candidate with target owner and domain handoff. Use `no_promotion` only when all candidates are rejected or intentionally not durable. Use `deferred_to_follow_up` when remaining work must move to follow-up Slice, backlog, domain intake, user handoff, or maintenance.
9. Produce `promotion.md` and `deferred.md` content or update proposals. If the current Runtime has no Slice artifact-write authority, return the proposed content and state that no canonical source was updated and no durable-domain storage was mutated.
10. Run an overreach scan before handoff: reject promotion without evidence, ownerless follow-up, result truth rewrites, direct durable-domain writes, branch/worktree cleanup execution, maintenance execution, deployment/live-system commands, hidden cleanup obligations, and Slice closure from promotion routing alone.

## Outputs
Outputs are Slice-local routing decisions and artifact content only.

`promotion.md` must include a result source block with `result.md`, highest validated truth, proof state, and forbidden-claim limits; a promotion candidate table with source artifact, claim or lesson, proof level, freshness, authority, sensitivity, target domain, target owner, required domain gate, next proof, and terminal route; a no-promotion/rejected table with source, reason, proof limit, and reopen condition; and a domain handoff block naming the domain pipeline intake while saying no canonical source was updated and no durable-domain storage was mutated.

`deferred.md` must include a result source block and a deferred/follow-up table with source artifact, deferred item, type, owner, first next action, required proof, target artifact or follow-up Slice, freshness/urgency, blocker, and route state. Cleanup obligations, user/team handoffs, proof gaps, maintenance requests, and domain intake must be explicit rows, never hidden inside a completion claim. The final output names one of `promotion_candidate_recorded`, `no_promotion`, or `deferred_to_follow_up` plus the next owning skill, service, domain pipeline, or human action.

## Verification
Verify the body by checking that it references `capabilities/pipelines/slice-variants/full-design-to-execution.pipeline.json`, includes at least one `docs/architecture/*.html` source, and keeps exactly the seven Layer 6B sections. Verify a routing output by confirming `result.md` exists, highest validated truth and proof state are not rewritten, every promotion or deferred row traces to a source artifact, every routed item has target owner or explicit blocker, every candidate has proof level/freshness/authority, and every durable-domain handoff names allowed gates instead of direct storage mutation.

Also verify that `promotion.md` and `deferred.md` expose no-promotion decisions, rejected ideas, cleanup obligations, proof gaps, and follow-up work; no local proof is presented as live proof; no canonical source was updated by this skill; and the route is exactly `promotion_candidate_recorded`, `no_promotion`, or `deferred_to_follow_up`. Deterministic checks are `node tools/validate-internal-skill-body-quality.mjs --skill tect-promotion-and-deferred-router` and `node tools/validate-internal-skill-trigger-fixtures.mjs --skill tect-promotion-and-deferred-router`.

## Failure Modes
Block when the parent Slice cannot be identified, `result.md` is absent, highest validated truth or proof state is missing, source artifacts contradict `result.md`, candidate sensitivity prevents normal routing, authority for a target owner is missing, or every proposed promotion depends on stale or unverified evidence. Do not normalize a route when promotion target, deferred owner, next proof, cleanup owner, domain gate, or no-promotion reason is unknown; leave a blocked route with the exact missing input.

Route back to result writing or proof-evidence audit when result truth is unstable, to verification/deployment/live validation when the route depends on missing proof, to the appropriate stateful-domain entry gate when promotion is ready for governed intake, to maintenance when only readiness/index/cleanup checks are needed, to a follow-up Slice when the target changes materially, and to handoff when the next owner is outside the current run. Never use this skill to promote directly, mutate durable KB/runbook/domain storage, rewrite `result.md`, execute cleanup or follow-up work, deploy, run live-system commands, or claim Slice closure from promotion routing alone.
