---
id: "slice-lightweight-escalation-checker"
kind: "internal_step"
owner_family: "slice_variant"
pipeline_id: "slice.lightweight-tdd-development"
step_id: "slice-lightweight-escalation-checker"
entry_gate: false
public_trigger: false
native_skill_discovery: false
body_path: "capabilities/instructions/pipelines/slice-variants/lightweight-tdd/slice-lightweight-escalation-checker.step.md"
source_manifest: "capabilities/pipelines/slice-variants/lightweight-tdd.pipeline.json"
legacy_skill_ref: "slice-lightweight-escalation-checker"
architecture_refs:
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#lightweight-development"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19"
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#lightweight-and-debug-slice-variants"
---

# Lightweight Escalation Checker

## Overview

This skill is the pause gate that decides whether a Lightweight TDD Slice can stay lightweight. The core rule is: lightweight means fewer artifacts, not weaker proof; any ambiguity, live/deploy consequence, missing proof path, or repeated failure must be routed before implementation continues.

## When to Use

Use after `slice-lightweight-contract-writer` has declared the lightweight Slice contract and before `slice-test-target-selector` or any continued lightweight step proceeds. Also use it when new evidence appears during a lightweight Slice and the agent must re-check whether the current path still fits.

Use for small understood code, config, fixture, docs, or business-rule changes where acceptance checks are clear, affected surface is bounded, and focused proof is expected to be enough. Do not use to choose tests, run TDD, patch source, verify implementation, decide deployment impact after local proof, write results, promote knowledge, execute operations, investigate unknown root cause, or run full design/spec work.

Do not select this skill for the first classification of a vague request, a current-state query, a stale-index repair, a research question, a result-writing step, a promotion decision, a deployment/live validation step, or an operations command. Those belong to Kernel/Runtime selection, query, maintenance, research-to-KB, result, promotion, hybrid, or ops pipelines. This checker only decides whether an already selected `slice.lightweight-tdd-development` path may continue.

## Source Contract

Grounding sources:

- `capabilities/pipelines/slice-variants/lightweight-tdd.pipeline.json` step `slice-lightweight-escalation-checker`
- `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#lightweight-development`
- `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#lightweight-and-debug-slice-variants`
- `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6`
- `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19`

The manifest step is required, invokes `slice-lightweight-escalation-checker`, produces `deferred.md`, gates on `escalation_triggers_checked`, reaches `lightweight_path_confirmed` or `escalation_required`, and fails through `escalate_full_debug_hybrid_or_block`. The relevant atom is `pipeline.slice.lightweight-tdd`, with manifest anchors `capabilities/pipelines/slice-variants/lightweight-tdd.pipeline.json#step_graph.steps.slice-lightweight-escalation-checker` and `capabilities/pipelines/slice-variants/lightweight-tdd.pipeline.json#step_graph.steps.slice-lightweight-escalation-checker.invokes.slice-lightweight-escalation-checker`.

Required source inputs for the check are the current `slice.md`, intent/request notes, acceptance checks, parent Scope constraints, context summary, workspace preflight summary, declared authority, proof contract, existing `deferred.md` if present, and any new evidence that appeared after the lightweight contract was written. If the Slice has no selected lightweight variant or no declared proof/authority contract, this skill blocks or routes back; it does not infer the missing contract.

## Operating Procedure

1. Load the current lightweight Slice contract, intent, acceptance checks, parent Scope constraints, context summary, workspace preflight status, declared authority, proof contract, existing deferrals, and any new evidence since the contract was written. If these inputs are missing, block or route back to the owning prior step instead of guessing.
2. Re-check lightweight fit. The path can continue only when the change is small, understood, bounded, non-operational, locally provable, and still has a single selected primary lifecycle under the Slice parent. Treat `lightweight` as lower ceremony only, never lower proof.
3. Check full-development triggers: architecture ambiguity, cross-component uncertainty, unclear requirements, hidden design decisions, broad refactor pressure, durable promotion risk, team/MR coordination, or a second lifecycle trying to appear inside the lightweight Slice.
4. Check debug triggers: observed behavior conflicts with expected behavior, root cause is unknown, the chosen acceptance proof fails for unexplained reasons, repeated local attempts fail, or the next honest step is reproduction/evidence rather than implementation.
5. Check hybrid or operational triggers: deploy or live validation becomes part of completion, production/runtime config is affected, data/security/user-visible consequence appears, rollback or release ownership matters, or an authorized operation is required.
6. Check research, result, and promotion triggers: the next honest work is evidence gathering, durable KB synthesis, result finalization, promotion readiness, stale knowledge correction, or procedure/runbook capture rather than a TDD implementation loop.
7. Check proof and authority triggers: no plausible test or acceptance proof target exists, authority is missing for the intended source/write/read action, source freshness is stale, privacy or sensitive data constraints block context use, or workspace/git risk invalidates the lightweight path.
8. Choose exactly one route. Keep lightweight only if every trigger class was checked and inactive. Route architecture ambiguity, broad design, cross-component uncertainty, missing proof target, or hidden second lifecycle to `slice.full-design-to-execution`. Route unknown root cause, unexplained failure, or repeated failure to `slice.debug-root-cause`. Route code/config work that needs deploy or live proof to hybrid implementation plus operation, `slice.hybrid-implementation-operation`. Route mutable operational work to operational preparation or operational execution based on authority. Route durable evidence work to research-to-KB, closure work to result, reusable/stale knowledge work to promotion, and unresolved authority or source gaps to handoff/blocker.
9. Emit one verdict. Use `lightweight_path_confirmed` only when every trigger class has been checked and no escalation is active. Use `escalation_required` when any trigger is active, naming the target route and stopping before test selection or implementation.
10. Write `deferred.md` as the checker record. Include checked trigger classes, evidence basis, active trigger or no-trigger verdict, chosen terminal state, target route, owner of next action, forbidden claims, and any deferred item that must survive result writing.

## Outputs

Primary output is `deferred.md` for the selected Lightweight TDD Slice. It must record the escalation check even when no escalation is active, with this shape:

```markdown
# Lightweight Escalation Check

- Slice: <slice id or path>
- Pipeline: slice.lightweight-tdd-development
- Checked sources: <slice.md, intent, acceptance, proof, authority, preflight, new evidence>
- Trigger matrix: <full/debug/hybrid/ops/research/result/promotion/proof/authority/freshness/workspace>
- Verdict: lightweight_path_confirmed | escalation_required
- Target route: slice-test-target-selector | slice.full-design-to-execution | slice.debug-root-cause | slice.hybrid-implementation-operation | operational-preparation/execution | research-to-KB | result | promotion | handoff/blocker
- Evidence basis: <why the verdict is valid now>
- Owner of next action: <agent, user, team owner, runtime, or blocked>
- Forbidden claims: <claims this Slice must not make>
- Deferred items: <items that must survive result writing>
```

When the path remains lightweight, the output must say `lightweight_path_confirmed` and route to `slice-test-target-selector` without creating heavyweight artifacts. When escalation is required, it must say `escalation_required`, name the route, preserve current lightweight artifacts as historical input, and stop before test selection or implementation.

This skill does not create `tdd-notes.md`, `test-plan.md`, `implementation-notes.md`, `verification.md`, `deployment-validation.md`, `result.md`, `promotion.md`, `handoff.md`, patches, commands, branches, deployments, live probes, or durable knowledge updates.

## Verification

Verify the trigger decision by checking that each escalation class was considered: full-development ambiguity, debug/root-cause uncertainty, hybrid or operational proof needs, research/result/promotion routing, proof-target gaps, authority gaps, freshness/privacy constraints, workspace risk, and deferred items. Confirm the proof gate `escalation_triggers_checked` is satisfied and the output terminal state is exactly `lightweight_path_confirmed` or `escalation_required`.

Run `node tools/validate-internal-skill-body-quality.mjs --skill slice-lightweight-escalation-checker` and `node tools/validate-internal-skill-trigger-fixtures.mjs --skill slice-lightweight-escalation-checker`. Also parse both fixture JSON files, scan headings for exactly `Overview`, `When to Use`, `Source Contract`, `Operating Procedure`, `Outputs`, `Verification`, and `Failure Modes`, confirm final newlines, scan owned files for trailing whitespace, and run `git diff --check -- skills/slice-lightweight-escalation-checker/SKILL.md validation/fixtures/internal-skill-body-quality/slice-lightweight-escalation-checker.json validation/fixtures/internal-skill-trigger/slice-lightweight-escalation-checker.json`.

## Failure Modes

Use `escalate_full_debug_hybrid_or_block` when the active Slice no longer satisfies lightweight constraints. Route to full design-to-execution for ambiguity or broad design decisions, debug/root-cause for unexplained failure or unknown cause, hybrid for code/config work whose completion depends on deploy or live proof, operational preparation/execution for mutable operations, research-to-KB for evidence work, result for closure work, and promotion for durable knowledge or reusable procedure work.

Block instead of continuing when required prior artifacts are missing, authority is undeclared, the proof target is absent, source truth is stale, workspace or git state invalidates safe continuation, privacy constraints prevent context use, or the escalation target cannot be chosen without user input. Do not downgrade proof, hide deferred work, create heavyweight artifacts without a typed escalation record, or let a lightweight result claim completion after an escalation trigger has appeared.
