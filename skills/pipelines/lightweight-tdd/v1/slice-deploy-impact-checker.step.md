---
id: "slice-deploy-impact-checker"
kind: "internal_step"
owner_family: "slice_variant"
pipeline_id: "slice.lightweight-tdd-development"
step_id: "slice-deploy-impact-checker"
entry_gate: false
public_trigger: false
native_skill_discovery: false
body_path: "capabilities/instructions/pipelines/slice-variants/lightweight-tdd/slice-deploy-impact-checker.step.md"
source_manifest: "capabilities/pipelines/slice-variants/lightweight-tdd.pipeline.json"
legacy_skill_ref: "slice-deploy-impact-checker"
architecture_refs:
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#lightweight-development"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19"
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#lightweight-and-debug-slice-variants"
---

# Slice Deploy Impact Checker

## Overview

This skill decides whether a lightweight TDD Slice can close with local proof, needs deployment or live validation, or must hand off to a user, team, hybrid Slice, or operational path. The core rule is lower ceremony, not lower proof: local verification is enough only when the changed surface has no deploy or live-behavior consequence.

It produces the deploy-impact decision for `deployment-validation.md` and gates `deploy_live_impact_checked`. Its boundary is evidence classification only: it does not perform deployment, does not validate live systems, does not write final result, does not promote the Slice, and does not mutate source.

## When to Use

Use this after `slice-lightweight-verification-runner` has recorded fresh local proof for `slice.lightweight-tdd-development` and before `slice-lightweight-result-writer` writes completion claims. Use it when the change touched deployable code, runtime config, package manifests, assets, user-visible behavior, API behavior, feature flags, infra/deploy files, dependency versions, migrations, or any surface where local-only evidence could be mistaken for deployed or live evidence.

Also use it to record an explicit `no_deploy_required` decision for docs-only, tests-only, local tooling, internal fixture, or non-runtime changes when the result would otherwise leave deploy impact implicit.

Do not use it to pick tests, run the RED/GREEN loop, gather local proof, execute a deployment, validate a live system after deployment, decide promotion, or write the final result. Escalate to full, hybrid, operational preparation, operational execution, or user/team handoff when the impact cannot be proven inside the lightweight contract.

## Source Contract

- Manifest step: `capabilities/pipelines/slice-variants/lightweight-tdd.pipeline.json#step_graph.steps.slice-deploy-impact-checker`.
- Step contract: produces `deployment-validation.md`, gates `deploy_live_impact_checked`, fails through `escalate_hybrid_or_handoff`, and exits as `no_deploy_required` or `deploy_validation_required`.
- Registry classification is skill_body for `tect-skill.slice-lightweight-debug.slice-deploy-impact-checker`.
- Architecture anchors: `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#lightweight-development`, `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#lightweight-and-debug-slice-variants`, `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6`, and `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19`.
- Atom anchors: `pipeline.slice.lightweight-tdd`, `pipeline.slice.lightweight_tdd.deploy.impact.checker`, and `capabilities/pipelines/slice-variants/lightweight-tdd.pipeline.json#step_graph.steps.slice-deploy-impact-checker.invokes.slice-deploy-impact-checker`.

The target record has no direct external skill reference. Keep the behavior Tect-owned and bounded by the current manifest, proof class, authority state, and parent Slice contract.

## Operating Procedure

1. Load the lightweight Slice contract, implementation notes, verification artifact, TDD notes or test plan, changed-file list, acceptance checks, parent Scope constraints, declared authority, and any release or deployment metadata already available. If fresh local proof is absent, route back to verification instead of deciding deploy impact.
2. Classify the touched surface. Separate docs/tests/local tooling from deployable application code, build/package changes, runtime configuration, infra or deployment files, public API behavior, user-visible behavior, data/storage changes, dependency changes, feature flags, migrations, and generated assets.
3. Compare proof class to claim class. The invariant is: local proof is not live proof. Local tests, builds, lint, typecheck, and local runtime checks can support local truth. They cannot support deployed, live, production-ready, user-visible, or completed-with-live-proof claims unless deployment and live evidence already exist.
4. Choose the terminal decision. Use `no_deploy_required` only when the changed surface cannot affect a deployed/runtime system or when deployment impact is already irrelevant to the Slice claim. Use `deploy_validation_required` when deployment, live behavior, release ownership, rollback, monitoring, environment readiness, or user/team returned proof is needed before final completion.
5. Check authority and ownership. If the agent lacks deploy authority, live access, environment ownership, rollback authority, or team integration permission, keep the decision as deploy-validation-needed and create a handoff route instead of silently downgrading the proof requirement.
6. Write or update `deployment-validation.md` with the decision, touched surfaces, evidence used, proof class, required deploy/live proof, owner or handoff target, forbidden claims, residual risk, and next route. The artifact may record that no deploy is required; it must not hide the check.
7. Route onward. Send `no_deploy_required` to lightweight result writing with local-only proof boundaries. Send `deploy_validation_required` to hybrid, operational preparation/execution, live-validation, or explicit user/team handoff according to authority and risk. Preserve `escalate_hybrid_or_handoff` when the decision cannot be made safely.

## Outputs

Produce `deployment-validation.md` for the selected lightweight Slice. It must include Slice id, changed surface classification, local proof reference, deploy-impact decision, proof class separation, required deployment or live evidence, owner or handoff target, blocked or missing authority, forbidden claims, residual risk, and the next manifest route.

Use `no_deploy_required` only with an explicit reason and evidence. Use `deploy_validation_required` when deploy/live validation or returned user/team proof is needed before the result can claim more than local verification.

## Verification

Before leaving this step, verify that `verification.md` exists, that `deployment-validation.md` names the touched surfaces and decision, and that every completion claim is limited to the available proof class. Confirm the artifact distinguishes local proof, deployment proof, live proof, and handoff proof; names any missing owner or authority; and records the chosen terminal state.

For skill-body validation, run:
- `node tools/validate-internal-skill-body-quality.mjs --skill slice-deploy-impact-checker`
- `node tools/validate-internal-skill-trigger-fixtures.mjs --skill slice-deploy-impact-checker`

## Failure Modes

Block or route through `escalate_hybrid_or_handoff` when local verification is missing, changed files cannot be classified, deploy topology is unknown, environment ownership is unclear, live proof is required but unavailable, authority is missing, rollback or monitoring responsibility is absent, or the change is broader than a lightweight Slice can safely contain.

Escalate when ambiguity, deploy risk, live risk, data migration risk, team coordination, repeated verification failure, or user-managed release ownership appears. Do not claim deployment success, live correctness, promotion readiness, or final completion from this skill alone.

Keep zero deploy or live success claims unless another authorized manifest step supplies the missing proof.
