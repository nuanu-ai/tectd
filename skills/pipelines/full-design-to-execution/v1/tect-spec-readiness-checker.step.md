---
id: "tect-spec-readiness-checker"
kind: "internal_step"
owner_family: "slice_variant"
pipeline_id: "slice.full-design-to-execution"
step_id: "tect-spec-readiness-checker"
entry_gate: false
public_trigger: false
native_skill_discovery: false
body_path: "capabilities/instructions/pipelines/slice-variants/full-design-to-execution/tect-spec-readiness-checker.step.md"
source_manifest: "capabilities/pipelines/slice-variants/full-design-to-execution.pipeline.json"
legacy_skill_ref: "tect-spec-readiness-checker"
architecture_refs:
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#full-development-slice-variant"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19"
---

# Tect Spec Readiness Checker

## Overview
This skill is the `skill_body` for the full design-to-execution readiness gate. Its core rule is to inspect the Slice design/spec chain and block false progress when the work is too broad, too abstract, missing actualized decisions, hiding deferred work, stale on current truth, or really belongs back at Scope.

## When to Use
Use this only for a `slice.full-design-to-execution` Slice at `readiness_gate`, after design shaping, component decision interrogation, cross-cutting review, reconciliation if needed, and synthesis have produced candidate inputs for planning.

Trigger when the next decision is whether the Slice can enter `implementation_ready` or must stop as `blocked_missing_decision`. Do not use it for initial problem framing, brainstorming options and tradeoffs, design-spec shaping, component extraction, dimension sweep, reconciliation, synthesis, implementation plan writing, RED/GREEN test work, subagent execution, code review, root cause debugging, branch or worktree setup, deployment, live validation, result writing, durable promotion, or maintenance repair.

## Source Contract
Ground this skill in:

- `capabilities/pipelines/slice-variants/full-design-to-execution.pipeline.json#step_graph.steps.slice-spec-readiness-checker`
- `capabilities/pipelines/slice-variants/full-design-to-execution.pipeline.json#capability_surface.skills.tect-spec-readiness-checker`
- `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#full-development-slice-variant`
- `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6`
- `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19`
- `docs/architecture/master-plugin-target-architecture-capability-atom-shard-map.html#pipeline.slice.full_design_to_execution.spec.readiness.checker`

The manifest step is `slice-spec-readiness-checker`, a required validator that invokes `tect-spec-readiness-checker` and emits `implementation_ready` or `blocked_missing_decision`. External/custom skills are reference inputs for expected upstream or downstream evidence only: spec-interrogation supplies the decisions directory, component extraction, dimension sweep, human decision, and README actualization pattern; spec-cross-cutting-review supplies cross-cutting review coverage; spec-reconciliation supplies contradiction repair status; spec-synthesis supplies the implementation-ready spec; Superpowers planning, subagent, TDD, verification, branch, review, and debugging skills define adjacent proof expectations and non-trigger routes.

## Operating Procedure
1. Confirm the Slice envelope. Require a bounded Slice/component target, parent Scope baseline or recovery link, current freshness basis, authority posture, and the active full-development artifact contract. If the target contains multiple independent Slices or still needs Scope decomposition, stop with `blocked_missing_decision`.
2. Inventory source inputs. Check for `slice.md` or equivalent Slice contract, `design-spec.md`, `decisions/README.md`, component decision files, `cross-cutting-review.md`, reconciliation evidence when findings exist, and `implementation-ready-spec.md`. Classify each as present, absent, stale, superseded, or out of scope.
3. Sweep the design spec. It must state the problem, component boundary, affected interfaces or behaviors, constraints, non-goals, assumptions, proof expectations, deployment or live-validation implications when relevant, and open questions. Broad intent without component-level shape is not ready.
4. Sweep decision interrogation evidence. Require actualized component decisions and human decision status: the README must match the component files, unresolved human-owned choices must be visible, feed-forward decisions must be represented, and waived or deferred decisions must name their owner and downstream route.
5. Sweep review and reconciliation evidence. Require cross-cutting review coverage for type, sequence, scope, data flow, amendment ripple, feed-forward, invariant, and integration consistency. If the review reports contradictions, gaps, stale references, or amendments, require reconciliation evidence showing resolved, deferred, or accepted-divergence status.
6. Sweep synthesis and planning handoff. The `implementation-ready-spec.md` candidate must preserve the original design as history, incorporate resolved decisions, name deferred work, and be concrete enough for `writing-plans` without inventing missing decisions. Do not create or edit the plan here.
7. Sweep proof, authority, and forbidden claims. Check that freshness, authority, required proof gates, and later deployment/live/handoff implications are explicit. Block unsupported completion and hidden deferred work; local proof, deployment proof, live proof, result writing, promotion, branch/worktree cleanup, and maintenance checks belong to later gates.
8. Emit the readiness verdict packet. Use `implementation_ready` only when planning can begin without conceptual invention. Use `blocked_missing_decision` with exact missing artifacts, stale evidence, unresolved decisions, contradictions, authority/freshness gaps, hidden deferrals, or Scope/variant-routing reason.

## Outputs
Return a readiness verdict packet for the owning Slice with these fields: Slice target and parent context, source inputs checked, artifact readiness table, decision status, human-decision queue status, cross-cutting review status, reconciliation status, implementation-ready spec status, explicit deferred items, proof/authority/freshness gaps, terminal state, next owner, and next route. Allowed terminal states are `implementation_ready` and `blocked_missing_decision`.

This skill writes no artifacts by default. It may only return the packet to the caller or propose the exact artifact that another step must create or repair. It must not mutate source repos, workspace control-plane files, branch/worktree state, deployment targets, durable knowledge, result records, promotion records, or maintenance state.

## Verification
Verify trigger fit by checking that positive scenarios are full design-to-execution Slices at the readiness gate, while negative scenarios ask for problem framing, design shaping, spec interrogation, reconciliation, synthesis, plan writing, implementation, deployment, live validation, result writing, promotion, query, maintenance, debugging, or another Slice variant. Verify body quality with `node tools/validate-internal-skill-body-quality.mjs --skill tect-spec-readiness-checker` and trigger coverage with `node tools/validate-internal-skill-trigger-fixtures.mjs --skill tect-spec-readiness-checker`. Also parse both owned fixtures as JSON and run scoped whitespace checks over this skill and its fixtures.

## Failure Modes
Return `blocked_missing_decision` when the Slice target is unbounded, parent Scope context is absent, current-state truth is stale, authority is unclear, `design-spec.md` is too abstract, component decisions are missing, human decisions remain unresolved, cross-cutting review is absent, reconciliation is incomplete, the synthesized spec is not implementation-ready, proof expectations are unclear, or deferred items are hidden.

Route upward when the work belongs to Scope, sideways when lightweight development, debug/root-cause, operational execution, research, or another Slice variant fits better, and downstream only after a clean readiness verdict. Refuse requests to synthesize the spec, write the plan, run implementation, deploy, validate live behavior, write the result, promote durable knowledge, clean branches/worktrees, or close the Slice from this gate.
