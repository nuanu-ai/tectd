---
id: "slice-research-index-front-door-checker"
kind: "internal_step"
owner_family: "slice_variant"
pipeline_id: "slice.research-to-durable-knowledge"
step_id: "slice-research-index-front-door-checker"
entry_gate: false
public_trigger: false
native_skill_discovery: false
body_path: "capabilities/instructions/pipelines/slice-variants/research-to-durable-knowledge/slice-research-index-front-door-checker.step.md"
source_manifest: "capabilities/pipelines/slice-variants/research-to-durable-knowledge.pipeline.json"
legacy_skill_ref: "slice-research-index-front-door-checker"
architecture_refs:
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#research-to-durable-kb-slice-variant"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19"
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#procedure-and-research-slice-variants"
---

# Research Index Front Door Checker

## Overview
This skill checks the discoverability boundary near the end of `slice.research-to-durable-knowledge`. It does not promote research, write canonical durable knowledge, update front doors, or rebuild indexes. It verifies that proposed, promoted, or restricted research can be found from the right discovery surfaces, or it records a routed deferral in `deferred.md`.

The core rule is zero mutation: a Research Slice may pass this step only when every promotion target is either discoverable now or has a documented owner, route, blocker, and stop condition. Missing discoverability is work to route, not truth to silently repair.

## When to Use
Use this skill only after the Research Slice has enough upstream material to compare against discovery surfaces:

- parent Program/Scope/Slice identity and active Research Slice front door;
- `durable-kb-seed.md` or equivalent durable object proposal;
- `promotion.md` with an approved, not-required, restricted, or blocked gate verdict, plus `promotion-edge.md` when present;
- target durable lane such as durable KB object page, runbook, protocol note, decision record, issue, follow-up Scope, or `index_or_front_door`;
- claim-ledger rows, source-provenance labels, freshness-authority labels, contradiction/gap status, and negative-knowledge references;
- known target owner, authority posture, and source/projection boundary.

Use it when the selected next gate is `index_front_door_checked_or_deferred`: the agent must decide whether the proposed or promoted knowledge is discoverable from the Slice front door, target durable-domain front door, canonical index/catalog, generated local index/projection, queryable current-state route, or handoff surface.

Do not use this skill to collect evidence, ingest sources, extract claims, build the claim ledger, resolve contradictions, write the KB seed, decide promotion authority, write promotion edges, edit canonical durable knowledge, update an index/front door, refresh a projection, execute maintenance, run live checks, deploy, or write the final result.

## Source Contract
The owning manifest is `capabilities/pipelines/slice-variants/research-to-durable-knowledge.pipeline.json`, step `slice-research-index-front-door-checker`. The exact manifest contract is: required skill step, produces `deferred.md`, gates `index_front_door_checked_or_deferred`, failure route `stop_or_handoff`, and terminal state `ready_for_next_step`.

Architecture anchors:

- `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#research-to-durable-kb-slice-variant`: research becomes durable only after evidence, provenance, freshness/authority labels, contradiction handling, promotion gate, and index/front-door checks.
- `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#procedure-and-research-slice-variants`: procedure and research variants cannot silently mutate durable runbooks, KB, skills, or plugin rules.
- `docs/architecture/master-plugin-target-architecture-part-6c-stateful-domain-pipelines.html#durable-kb-pipeline`: durable-domain pipelines own canonical storage, indexes, query refresh, repair candidates, and accepted/rejected/partial results.
- `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6`: Research to durable KB is a Slice variant whose completion boundary is promotion/no-promote/blocked, not raw research output.
- `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19`: selection is two-stage; public skills are front doors, Runtime materializes manifest steps, and Research -> durable KB writes only through target pipeline or owner.
- `docs/architecture/master-plugin-target-architecture-capability-atom-shard-map.html#pipeline.slice.research_to_durable_knowledge.index.front.door.checker`: atom `pipeline.slice.research_to_durable_knowledge.index.front.door.checker`, normalized skill `slice-research-index-front-door-checker`, required/allowed under `tect-work`.

Registry/status rows resolve this as an Tect-owned skill with no external references and with all side-effect permissions false. Layer 6b readiness is not fidelity proof; this body must stand alone.

## Operating Procedure
1. Confirm trigger fit. Require a Research To Durable Knowledge Slice, upstream seed or promotion packet, target lane, promotion status, claim/provenance/freshness/contradiction basis, target owner, and authority posture. If those are missing, route backward to the owning upstream step instead of checking discoverability.
2. Build the check packet. Record source inputs: Slice front door path, proposed object title/id/path, promotion target, accepted/restricted/rejected claim rows, source links, freshness class, authority basis, contradiction or negative-knowledge notes, target owner, and current handoff route.
3. Resolve discoverability targets. Check only targets relevant to the promotion packet: Slice front door, Scope/Program front door when it must route the finding, durable-domain front door, canonical domain index/catalog, promotion log, generated local index/projection, queryable current-state summary, and handoff surface.
4. Classify source class before comparing. Canonical durable KB pages, runbooks, protocol notes, decision records, issues, and follow-up Scopes are owned by their domain pipeline or object owner. Generated indexes, query summaries, and local projections are rebuildable projections. This step may inspect or propose wording; it may not apply the change.
5. Compare each target against the promotion packet. Check object title/id/path, claim status, promotion status, restrictions, freshness class, contradiction status, source/provenance links, target owner, route hint, and whether future query can find the highest validated research truth without reading chat history.
6. Classify each target as `current`, `update_required`, `index_update_proposal_required`, `front_door_update_proposal_required`, `derived_index_refresh_required`, `repair_proposal_required`, `blocked_by_authority`, `blocked_by_missing_target`, `deferred_to_domain_owner`, or `out_of_scope_for_this_promotion`.
7. Write the final `deferred.md` for every promotion verdict, including `promotion_blocked`. For every non-current target, include the stale or missing surface, mismatch, proposed route hint or wording, canonical versus projection boundary, required owner, authority basis, blocker, stop condition, and handoff route. For current or not-required targets, record the evidence that no update proposal is needed. This step is the sole writer of `deferred.md`.
8. Decide the proof gate. Pass `index_front_door_checked_or_deferred` only when every in-scope target is either `current`, explicitly out of scope, or represented in `deferred.md` with complete owner/blocker/route details. Add exact line `gate: index_front_door_checked_or_deferred` only then. If any target lacks proof, owner, authority, target identity, or source/projection boundary, leave the gate blocked.
9. Route next work without performing it. Send final Slice closure to `slice-research-result-and-handoff-writer`; canonical durable KB writes to the durable KB domain pipeline; runbook/procedure updates to the runbook library pipeline; protocol/product/security/operations/devops targets to their domain pipeline; generated projection refresh to maintenance/index rebuild; stale or contradictory front doors to maintenance/front-door sync or repair proposal; missing authority to user/team handoff; broader discoveries to Program/Scope/Slice decomposition.

## Outputs
The only manifest-declared artifact is `deferred.md`, and this step is its sole owner. Write or supply this section shape for inclusion there:

```yaml
index_front_door_verdict:
  gate: index_front_door_checked_or_deferred
  gate_status: pass | blocked
  promotion_packet:
    slice_id:
    promotion_status:
    promotion_target:
    proposed_object:
    claim_status_summary:
    freshness_class:
    authority_basis:
    contradiction_status:
  checked_targets:
    - target:
      source_class: slice_front_door | program_scope_front_door | canonical_domain_index | durable_domain_front_door | generated_projection | query_summary | handoff_surface
      status: current | update_required | index_update_proposal_required | front_door_update_proposal_required | derived_index_refresh_required | repair_proposal_required | blocked_by_authority | blocked_by_missing_target | deferred_to_domain_owner | out_of_scope_for_this_promotion
      mismatch:
      discoverability_target:
      route_hint:
      canonical_versus_projection_boundary:
      required_proposal:
      next_owner:
      blocker:
      stop_condition:
  handoff_routes: []
  claims_blocked_until: []
```

Valid terminal outcomes for this step are `ready_for_next_step` when the gate passes, `stop_or_handoff` when missing proof or authority prevents a safe verdict, or a blocked/deferred Research Slice terminal posture preserved from upstream: `durable_kb_seed_proposed`, `promoted_to_durable_kb`, `promoted_with_restrictions`, `handoff_for_human_review`, `escalated_to_durable_domain_pipeline`, `blocked_by_freshness_or_authority`, `blocked_by_contradiction`, or `rejected_insufficient_evidence`. This skill never creates those promotions; it reports whether discoverability is current, proposed, deferred, or blocked.

## Verification
Verify source compliance by checking the manifest step above, the Part 6B research/procedure boundaries, the Part 6C durable KB boundary, final map `#s6` and `#s19`, the capability atom row, and the registry/status side-effect boundary. The skill body must keep the seven required sections: Overview, When to Use, Source Contract, Operating Procedure, Outputs, Verification, and Failure Modes.

Verify behavior with positive cases:

- a promoted durable KB seed whose Slice front door, durable-domain front door, canonical index, and query route are already current, yielding `gate_status: pass` and no required proposal;
- a restricted promotion whose generated local index is stale and whose durable-domain owner must update a front door later, yielding a deferred route with owner and blocker;
- a seed-only proposal where no canonical entry exists yet, yielding `index_update_proposal_required` or `deferred_to_domain_owner` without claiming canonical durable truth.

Verify negative cases:

- missing KB seed, promotion decision, promotion edge, provenance, freshness label, contradiction review, target owner, or authority posture routes backward or blocks;
- direct requests to edit durable KB, update front doors, rebuild indexes, execute maintenance, deploy, run live checks, mutate branches/worktrees, or promote research are rejected as non-triggers;
- any stale target without owner, blocker, route, or stop condition blocks `index_front_door_checked_or_deferred`.

Run `node tools/validate-internal-skill-body-quality.mjs --skill slice-research-index-front-door-checker` and `node tools/validate-internal-skill-trigger-fixtures.mjs --skill slice-research-index-front-door-checker`. Also parse both owned JSON fixtures, scan the owned files for trailing whitespace and final newlines, and run scoped `git diff --check` over the owned files.

## Failure Modes
Route backward when the Slice lacks `durable-kb-seed.md`, `promotion.md`, promotion edge or gate decision, source-provenance labels, freshness-authority labels, claim-ledger rows, contradiction/gap review, negative-knowledge references, target durable lane, target owner, or authority posture.

Use deferred routing, not mutation, when a canonical front door is stale, a durable-domain index needs owner action, a generated index needs refresh, a route hint is missing, a query summary is outdated, the target belongs to a domain pipeline, or authority to edit is absent. Preserve the blocker in `deferred.md` so the result writer reports the highest validated truth without implying that discoverability was repaired.

Reject or hand off requests for durable KB writes, runbook/protocol/product/security/operations/devops canonical writes, index/front-door mutation, projection rebuild execution, maintenance execution, researched external code execution, deployment or live-system commands, branch/worktree mutation, package execution, team merge, or direct durable promotion. The required final posture is no durable KB write, no index mutation, no durable promotion, and no stronger completion claim than the proof gate supports.
