---
id: "tect-design-spec-shaper"
kind: "internal_step"
owner_family: "slice_variant"
pipeline_id: "slice.full-design-to-execution"
step_id: "tect-design-spec-shaper"
entry_gate: false
public_trigger: false
native_skill_discovery: false
body_path: "capabilities/instructions/pipelines/slice-variants/full-design-to-execution/tect-design-spec-shaper.step.md"
source_manifest: "capabilities/pipelines/slice-variants/full-design-to-execution.pipeline.json"
legacy_skill_ref: "tect-design-spec-shaper"
architecture_refs:
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#full-development-slice-variant"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19"
---

# Tect Design Spec Shaper

## Overview
This is a standalone Tect-owned design-spec shaping skill record for the `slice.full-design-to-execution` capability surface. It adapts the useful parts of `superpowers:brainstorming` - problem framing, options, and tradeoff exploration - into a Slice-local `design-spec.md` payload without copying brainstorming's approval workflow or taking over later Slice steps.

Core rule: shape exploration into design-spec material only; keep uncertainty, proof needs, routing boundaries, and forbidden downstream actions visible.

## When to Use
- Use when Runtime has selected the full design-to-execution Slice variant and the manifest path `capabilities/pipelines/slice-variants/full-design-to-execution.pipeline.json` invokes `tect-design-spec-shaper` from the `slice-design-spec-shaper` manifest step.
- Use when parent Scope context, Slice boundary, current evidence, and brainstorming notes need to become a coherent `design-spec.md` draft or patch-ready sections.
- Use when notes contain mixed problem framing, candidate options, tradeoffs, assumptions, constraints, non-goals, dependency risks, proof expectations, deployment or live-validation implications, and unresolved decisions.
- Use before component decision interrogation, while the work still needs a design-spec shape rather than per-component decision answers.
- Do not use for Program strategy, Scope decomposition, target discovery, lightweight TDD, debug/root-cause, operational preparation, operational execution, hybrid implementation-ops, research-to-durable-knowledge, or procedure capture.
- Do not use to perform component decisions, build `decisions/*.md`, write an implementation plan, execute code, deploy, write `result.md`, promote durable knowledge, or mutate workspace state.

## Source Contract
Grounding sources:
- `capabilities/pipelines/slice-variants/full-design-to-execution.pipeline.json`: capability surface skill `tect-design-spec-shaper`; manifest step `slice-design-spec-shaper` invokes it and declares the `design_gate`, terminal states `design_ready` and `blocked_missing_context`, and failure routes that block false completion or hand off when target material changes.
- `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#full-development-slice-variant`: full development Slice lifecycle, required artifacts, forbidden repeated execution folders, no promotion without a transition record, and no result claim beyond proof.
- `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6`: Slice/Component artifact shapes, proof classes, terminal behavior, and the rule that implementation execution is not Slice completion.
- `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19`: variant selection, runtime materialization, escalation and de-escalation rules, and the distinction between Slice parent ownership and variant-owned internal steps.
- `docs/architecture/master-plugin-target-architecture-capability-atom-shard-map.html#pipeline.slice.full_design_to_execution.design.spec.shaper`: atom row mapping the design-spec shaper to the full design-to-execution manifest, Scope baseline inputs, `design-spec.md` outputs, proof requirement, and fallback route.

Reference source: `skills/references/superpowers/brainstorming/SKILL.md`. Use it only as reference for context exploration, problem framing, candidate options, tradeoff comparison, and approval awareness. Tect owns the artifact shape, terminal states, routing, proof boundary, and handoff rules.

## Operating Procedure
1. Confirm selection fit. Verify the selected work is a bounded Slice/Component inside `slice.full-design-to-execution`, with parent Scope baseline, current evidence, authority posture, and proof expectations available. If the target is above Slice level or belongs to another variant, stop and route instead of shaping a false spec.
2. Load source inputs. Read or receive the Slice intent, Scope decomposition, current-system baseline, constraints, non-goals, assumptions, evidence freshness notes, user decisions already made, brainstorming exploration, candidate options, tradeoffs, and known proof or deployment implications.
3. Normalize problem framing. Convert raw notes into a concise Slice problem statement, target behavior, user/business reason, affected components or interfaces at a high level, and current evidence basis. Mark stale, inferred, or unverified claims explicitly.
4. Preserve options and tradeoffs. Keep candidate options separate from the selected direction. For each serious option, capture why it exists, tradeoffs, risks, proof implications, and rejection or pending reason. Choose a direction only when evidence and constraints justify it; otherwise record the exact decision questions.
5. Shape the `design-spec.md` artifact. Produce patch-ready sections with this output shape: purpose, source inputs, current baseline, target behavior, component/interface boundary, constraints, non-goals, assumptions, candidate options, tradeoffs, selected or pending direction, proof expectations, deployment/live-validation implications, unresolved decisions, handoff notes, and readiness verdict.
6. Guard downstream boundaries. Do not answer component interrogation questions, create decision files, choose implementation files, write plan tasks, run commands, deploy, validate live behavior, write result/promotion artifacts, or promote durable knowledge. Name those as later manifest responsibilities.
7. Emit terminal state. Return `design_ready` only when the design-spec payload is concrete enough for component decision interrogation. Return `blocked_missing_context` with exact missing inputs when Scope baseline, Slice boundary, evidence, authority, proof expectations, or unresolved decisions are too vague.
8. Hand off deliberately. On `design_ready`, hand the shaped `design-spec.md` payload to the next full-development design gate. On route-away, name the correct owner: Program, Scope, lightweight TDD, debug/root-cause, operational prep/execution, hybrid implementation-ops, research-to-durable-knowledge, or procedure capture. On material target change, hand off or request a follow-up Slice.

## Outputs
- A patch-ready `design-spec.md` payload or section set using the artifact shape defined above. The caller may persist it only when the enclosing Slice step has canonical Slice write authority; this skill itself does not claim durable workspace write authority.
- Candidate options and tradeoffs kept distinct from the chosen or pending direction, with assumptions, non-goals, stale evidence, proof gaps, deployment/live-validation implications, and unresolved decisions preserved.
- A terminal state: `design_ready`, `blocked_missing_context`, or a route-away handoff note naming the correct owner and reason.
- Handoff notes for the next gate: component decision interrogation inputs when ready, or exact missing context and proof gaps when blocked.
- Forbidden actions remain forbidden: component decision authority, implementation plan authority, code execution authority, deployment authority, result authority, promotion authority, and durable workspace write authority.

## Verification
- Body-quality check: `node tools/validate-internal-skill-body-quality.mjs --skill tect-design-spec-shaper` must pass and prove the body has Layer 6B sections, the owning manifest path, architecture HTML anchors, concrete operating procedure, `design-spec.md` artifact shape, and adapted `superpowers:brainstorming` markers for problem framing, options, and tradeoff behavior.
- Trigger check: `node tools/validate-internal-skill-trigger-fixtures.mjs --skill tect-design-spec-shaper` must pass with at least two positive standalone skill-record scenarios and one negative route-away scenario.
- Content review: shaped output must separate exploration from direction, declare source inputs, preserve assumptions and unresolved decisions, include proof/deployment/live-validation expectations, expose terminal state, and stay inside the Slice abstraction boundary.
- Boundary review: no output may claim component decisions, implementation planning, code execution, deployment, live validation, result writing, promotion, or durable workspace mutation.

## Failure Modes
- Missing parent Scope baseline, Slice boundary, current evidence, authority posture, proof expectations, or unresolved-decision owner: return `blocked_missing_context` and list the exact missing source input.
- Target belongs to Program or Scope work: route upward and preserve the reason.
- Target fits another Slice variant: route to lightweight TDD, debug/root-cause, operational prep/execution, hybrid implementation-ops, research-to-durable-knowledge, or procedure capture.
- Brainstorming output contains unsupported options: keep them as candidate options or unresolved decisions; do not present preference as chosen direction.
- Deployment or live validation defines completion but is unclear: mark it as a proof gap and block handoff that would overclaim readiness.
- User asks this skill to decide components, plan, code, run commands, deploy, write results, promote, or persist durable state: refuse within this skill boundary and hand off to the proper manifest gate or authority owner.

## Core Pattern
Problem framing plus options and tradeoffs become `design-spec.md` sections; unsupported exploration remains visibly unresolved until later full-development gates decide.
