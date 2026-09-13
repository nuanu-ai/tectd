---
id: "tect-full-dev-entry-gate"
kind: "internal_step"
owner_family: "slice_variant"
pipeline_id: "slice.full-design-to-execution"
step_id: "tect-full-dev-entry-gate"
entry_gate: true
public_trigger: false
native_skill_discovery: false
body_path: "capabilities/instructions/pipelines/slice-variants/full-design-to-execution/tect-full-dev-entry-gate.step.md"
source_manifest: "capabilities/pipelines/slice-variants/full-design-to-execution.pipeline.json"
legacy_skill_ref: "tect-full-dev-entry-gate"
architecture_refs:
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#full-development-slice-variant"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19"
---

# Tect Full Dev Entry Gate

## Overview
This skill is the standalone entry gate for selecting the full design-to-execution Slice variant. Its core rule is to admit only a bounded Slice candidate whose parent context, authority, freshness, proof posture, and lifecycle depth justify the heavy full-development path.

## When to Use
Use this after `tect-work`, Kernel classification, Runtime family selection, and the Slice parent route have narrowed the request to a concrete Slice or component candidate, including cases where the gate must block because required parent or boundary inputs are missing. Trigger signals include high ambiguity, architecture-sensitive design, cross-component impact, unresolved human decisions, deployment or live-validation implications, durable promotion risk, or proof needs that exceed a focused lightweight TDD, debug, operations, research, procedure, query, maintenance, setup, or adoption route.

Do not use this for Program or Scope discovery, broad decomposition, current-state lookup, stale projection repair, setup/adoption, durable-domain grooming, small clear patches, root-cause debugging, operational preparation, operational execution, hybrid code-plus-live work, research-to-KB, procedure capture, or any full-development Slice that has already entered design shaping, contract writing, planning, execution, verification, deployment, result, promotion, or maintenance. Route those cases to the owning parent, sibling variant, service, maintenance capability, or downstream full-development step.

## Source Contract
Ground this skill in:

- `capabilities/pipelines/slice-variants/full-design-to-execution.pipeline.json#applicability`
- `capabilities/pipelines/slice-variants/full-design-to-execution.pipeline.json#entry_contract`
- `capabilities/pipelines/slice-variants/full-design-to-execution.pipeline.json#capability_surface.skills.tect-full-dev-entry-gate`
- `capabilities/pipelines/slice-variants/full-design-to-execution.pipeline.json#step_graph.steps.slice-full-dev-entry-gate.invokes.tect-full-dev-entry-gate`
- `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#full-development-slice-variant`
- `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6`
- `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19`
- `docs/architecture/master-plugin-target-architecture-capability-atom-shard-map.html#pipeline.slice.full_design_to_execution.entry.gate`

The owning manifest step is `slice-full-dev-entry-gate`; the invoked Tect skill reference is `tect-full-dev-entry-gate`. Required parent and variant inputs are `parent_scope_candidate`, `slice_parent_variant_integration`, `pipeline_variant_selection_record`, `kernel_assessment`, `authority_state`, `freshness_state`, and `full_variant_artifact_contract`. Required evidence is `scope_baseline`, `component_boundary`, `current_workspace_state_or_declared_unknown`, `source_provenance`, and `artifact_contract_ref`. No external skill body is canonical for this gate.

## Operating Procedure
1. Confirm the parent route. Require a parent Program/Epoch/Scope link or explicit recovery path, plus a Slice parent context that can own target, selected variant, artifact contract instance, proof contract, and result boundary. If this is still Program/Scope discovery, route upward instead of entering this gate.
2. Confirm the Slice boundary. Name the exact repo, product area, component, behavior, interface, or workflow under consideration. Block vague initiatives, multi-Slice bundles, unrelated second targets, and missing component boundaries with `blocked_missing_parent_scope`.
3. Check required inputs. Verify `parent_scope_candidate`, `slice_parent_variant_integration`, `pipeline_variant_selection_record`, `kernel_assessment`, `authority_state`, `freshness_state`, and `full_variant_artifact_contract` are present or explicitly declared missing. Missing parent, boundary, authority, freshness, provenance, proof, or artifact-contract input is a blocker, not permission to guess.
4. Test variant fit. Select full design-to-execution only when ambiguity, architecture impact, cross-component dependency, human decision load, deployment or live proof, durable promotion risk, or proof complexity makes narrower variants insufficient. Small clear work belongs to lightweight TDD; unknown-cause behavior belongs to debug/root-cause; operation-only work belongs to operational preparation or execution; code plus live operation belongs to hybrid; evidence corpus work belongs to research-to-KB; repeated ad hoc workflow discovery belongs to procedure capture.
5. Reject wrong variants explicitly. For every rejected alternative, record the target route and reason: wrong abstraction, wrong Slice variant, service/query/maintenance/adoption path, missing authority, stale truth, missing setup, missing privacy clearance, or downstream step already owning the active Slice.
6. Activate the full Slice artifact contract without claiming artifacts are present. The active output shape is `design-spec.md`, `decisions/`, `cross-cutting-review.md`, `implementation-ready-spec.md`, `implementation-plan.md`, `execution.md`, `verification.md`, `deployment-validation.md`, `result.md`, `promotion.md`, and `deferred.md`; this gate only makes that contract applicable for the next owner.
7. Emit the entry verdict. Use `entry_ready` only when the parent link, bounded Slice target, authority and freshness posture, source provenance, proof expectations, artifact contract, and full-depth rationale are explicit enough for downstream design/spec shaping. Use `blocked_wrong_variant` for variant mismatch. Use `blocked_missing_parent_scope` for missing parent, target, boundary, authority, freshness, source provenance, proof basis, or artifact contract.
8. Stop at the gate boundary. Produce an entry packet, blocker packet, or reroute packet only. Avoid downstream artifact creation, source mutation, execution, deployment, live validation, result writing, promotion, and closure; those actions belong to later manifest steps after a valid entry packet exists.

## Outputs
Emit a full-development entry packet with `terminal_state`, `parent_program_epoch_scope_or_recovery`, `slice_target`, `component_boundary`, `kernel_and_runtime_basis`, `variant_fit_signals`, `authority_state`, `freshness_state`, `source_provenance`, `proof_and_deploy_implications`, `artifact_contract_ref`, `activated_artifact_shape`, `rejected_alternatives`, `missing_inputs`, `handoff_or_reroute`, and `next_owner`. Allowed terminal states are `entry_ready`, `blocked_wrong_variant`, and `blocked_missing_parent_scope`. A blocked packet must name the exact missing parent, boundary, authority, freshness, proof, source, artifact-contract, or routing input needed before retry.

## Verification
Verify trigger fidelity by checking that positive cases either have a bounded Slice candidate ready for the full-development entry decision or need this gate to report missing parent/boundary inputs for a concrete attempted Slice. Negative cases must route to Program/Scope parent work, lightweight TDD, debug/root-cause, operational preparation/execution, hybrid, research, procedure capture, query, maintenance, setup/adoption, or a downstream full-development step that already owns the active Slice. Verify content with `node tools/validate-internal-skill-body-quality.mjs --skill tect-full-dev-entry-gate`, trigger coverage with `node tools/validate-internal-skill-trigger-fixtures.mjs --skill tect-full-dev-entry-gate`, JSON parsing of both fixtures, a direct trailing-whitespace scan over the skill and fixtures, and `git diff --check -- skills/tect-full-dev-entry-gate/SKILL.md validation/fixtures/internal-skill-body-quality/tect-full-dev-entry-gate.json validation/fixtures/internal-skill-trigger/tect-full-dev-entry-gate.json`.

## Failure Modes
Return `blocked_missing_parent_scope` when the parent Program/Epoch/Scope link, recovery route, bounded Slice target, component boundary, authority posture, freshness basis, source provenance, proof expectation, or artifact-contract reference is missing, stale, or contradictory. Return `blocked_wrong_variant` when Kernel or Runtime signals a smaller TDD patch, debug/root-cause investigation, operation-only path, hybrid code/live path, research corpus task, procedure-capture candidate, maintenance repair, setup/adoption need, durable-domain task, read-only query, privacy-restricted route, or already-active downstream full-development step. Handoff when another owner must refresh truth, grant authority, restore parent Scope context, split a target, or continue from an existing downstream artifact. Do not perform design shaping, contract writing, planning, execution, verification, deployment, result, promotion, maintenance, cleanup, source mutation, worktree mutation, or live-system commands from this gate.
