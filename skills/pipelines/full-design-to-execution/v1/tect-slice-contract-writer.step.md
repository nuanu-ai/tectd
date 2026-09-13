---
id: "tect-slice-contract-writer"
kind: "internal_step"
owner_family: "slice_variant"
pipeline_id: "slice.full-design-to-execution"
step_id: "tect-slice-contract-writer"
entry_gate: false
public_trigger: false
native_skill_discovery: false
body_path: "capabilities/instructions/pipelines/slice-variants/full-design-to-execution/tect-slice-contract-writer.step.md"
source_manifest: "capabilities/pipelines/slice-variants/full-design-to-execution.pipeline.json"
legacy_skill_ref: "tect-slice-contract-writer"
architecture_refs:
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#full-development-slice-variant"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19"
---

# Tect Slice WorkOrder Contract Writer

## Overview
This internal instruction authors the executable route contract for ordinal 4 of `slice.full-design-to-execution`. Its only authorable carrier is `work-order-contract.json` with contract kind `workspace_work_order_route_contract_v1`. The already-authored `slice.md` and completed `design-spec.md` are inputs, never outputs of this step.

## When to Use
Use only when Runtime selected `slice.full-design-to-execution`, the current manifest step is `slice-contract-writer`, ordinals 1 through 3 are satisfied, and the Slice needs the exact WorkOrder authority contract required before later planning or execution.

Do not use this instruction to create or revise `slice.md`, `design-spec.md`, decisions, reviews, reconciliation, implementation specifications or plans, execution or verification evidence, deployment records, results, promotion, deferred work, maintenance, or handoff. Those artifacts belong to their owning steps.

## Source Contract
- Owning manifest: `capabilities/pipelines/slice-variants/full-design-to-execution.pipeline.json`, step `slice-contract-writer`.
- Canonical schema and exact-key template: `capabilities/registry/workspace-onboarding-v1.json#first_work_lifecycle.slice_admission.contract_template`.
- Canonical carrier: `{slice_root}/work-order-contract.json`.
- Required contract kind: `workspace_work_order_route_contract_v1`.
- Gate: `artifact_contract_gate`.
- Terminal states: `contract_ready` or `blocked_missing_artifact_contract`.
- Architecture anchors: `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#full-development-slice-variant`, `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6`, and `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19`.

## Operating Procedure
1. Confirm the runtime boundary. Continue only for the exact full-design Slice route and current step `slice-contract-writer`; otherwise stop and return to Runtime routing.
2. Read the canonical Slice inputs: `slice.md`, completed `design-spec.md`, parent Scope context, current source files that execution will rely on, selected route identity, authority posture, and explicit product write targets. Do not invent missing targets or authority.
3. Copy the exact `contract_template` object from `workspace-onboarding-v1.json#first_work_lifecycle.slice_admission`. Substitute every placeholder and preserve its exact keys; extra descriptive, workspace, or contract-id fields are invalid.
4. Bind `route_target_ref` to the exact stable Slice ref and choose one stable `member_id`. Bind every `required_reads` row to a current workspace-relative source path with `read_kind = source_path`, `required = true`, and the SHA-256 content hash of the bytes actually read. At minimum, bind the current `slice.md` and `design-spec.md` when both govern execution.
5. Bound `allowed_write_scope` to the Slice evidence paths and explicit product targets only. Every `allowed_paths` entry must equal or descend from an `allowed_roots` entry. Keep `.tect`, `.git`, and `tect/workspace` denied; keep `before_hash_required = true`. This instruction records authority but does not grant broader source, command, deployment, or live-system authority.
6. Preserve the template's operation class, candidate families, proof requirements, validation requirements, `result_closure`, `refresh_resume`, action and execution-envelope refs, and derived leaf fields. Replace placeholders with observable, bounded requirements; never weaken result and handoff closure.
7. Validate the completed JSON before writing: exact keys only, canonical contract kind, stable route/member identity, current hashed reads, bounded write scope, denied runtime/private roots, result and handoff paths under the Slice root, and refresh/resume enabled. If any required fact is missing or stale, return `blocked_missing_artifact_contract` and name it without fabricating a contract.
8. Write or update only `{slice_root}/work-order-contract.json`, then hand off to the next current full-design step. `contract_ready` means only that executable authority is declared; it does not mean design review, planning, implementation, verification, deployment, result closure, or promotion is complete.

## Outputs
The only output is `work-order-contract.json`, exactly matching `workspace_work_order_route_contract_v1` and the canonical exact-key template. No Markdown lifecycle artifact is authored by this step.

## Verification
Verify that `work-order-contract.json`:

- parses as JSON and has no keys outside the canonical template;
- names the exact Slice route and stable member;
- binds current workspace-relative required reads to their actual SHA-256 hashes;
- limits writes to declared Slice and explicit product targets while denying runtime/private roots;
- preserves candidate, proof, validation, result-closure, refresh, and resume obligations; and
- is exactly the artifact named by the manifest step `produces`, ordinal-4 completion predicate, required reads, artifact contract, and atom output.

Run:
- `node tools/validate-internal-skill-body-quality.mjs --skill tect-slice-contract-writer`
- `node tools/validate-internal-skill-trigger-fixtures.mjs --skill tect-slice-contract-writer`

## Failure Modes
Use `blocked_missing_artifact_contract` when route identity, current required reads, product write targets, authority, proof requirements, validation requirements, or result closure cannot be stated from current source truth. Route back to Runtime when another variant or step owns the request.

Forbidden actions: writing `slice.md` or any downstream lifecycle artifact; inventing or widening paths or authority; omitting content hashes; adding non-template keys; mutating product source; running implementation or product validation; creating or cleaning worktrees; installing packages; deploying; performing live checks; closing results; promoting; or claiming that contract readiness equals work completion.
