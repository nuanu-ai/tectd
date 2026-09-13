---
id: "slice-debug-context-loader"
kind: "internal_step"
owner_family: "slice_variant"
pipeline_id: "slice.debug-root-cause"
step_id: "slice-debug-context-loader"
entry_gate: false
public_trigger: false
native_skill_discovery: false
body_path: "capabilities/instructions/pipelines/slice-variants/debug-root-cause/slice-debug-context-loader.step.md"
source_manifest: "capabilities/pipelines/slice-variants/debug-root-cause.pipeline.json"
legacy_skill_ref: "slice-debug-context-loader"
architecture_refs:
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#debug-root-cause-variant"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19"
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#lightweight-and-debug-slice-variants"
---

# Slice Debug Context Loader

## Overview
Load the smallest current context packet needed for a `slice.debug-root-cause` investigation. The core rule is that context comes before diagnosis or fixes: load evidence-bearing sources, mark stale or missing context, and stop at `context_ready` only when downstream debug steps can proceed without guessing.

This is an Tect-owned internal Slice step. It executes after `slice-debug-entry-gate` and before the debug contract, symptom, reproduction, evidence-order, hypothesis, root-cause, fix, verification, result, promotion, or handoff steps. Its only durable output is context status for the selected debug Slice.

## When to Use
- Use after `slice-debug-entry-gate` has accepted a bug, regression, failed verification, or unexplained behavior as a debug/root-cause Slice.
- Use when the current turn lacks enough source context to start symptom capture, reproduction, evidence ordering, recent-change inspection, data-flow tracing, or hypothesis work.
- Use for code, tests, logs, traces, recent diffs or commits, config/runtime state, prior reproduction attempts, parent Scope constraints, proof obligations, and stale or contradictory context.
- Do not use for a small understood fix, full design work, live incident execution, deployment recovery, root-cause declaration, test writing, patching, result writing, promotion, or durable domain updates.

## Source Contract
- Manifest step: `capabilities/pipelines/slice-variants/debug-root-cause.pipeline.json`, `step_graph.steps.slice-debug-context-loader`; required gates are `current_context_loaded` and `stale_context_marked`, terminal state is `context_ready`, and failure routes to `request_context_or_block`.
- Architecture: `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#debug-root-cause-variant` and `#lightweight-and-debug-slice-variants`, plus `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6` and `#s19`.
- Atom map: `docs/architecture/master-plugin-target-architecture-capability-atom-shard-map.html` row `pipeline.slice.debug_root_cause.context.loader`, which maps this step to loading code, tests, logs, recent diffs/commits, runtime state, and parent constraints.
- Required inputs: accepted debug Slice target, observed-vs-expected failure signal, parent Scope or Slice constraint pointer, authority/read limits, current workspace/repo target, and any available logs, tests, diffs, runtime state, prior notes, or blocked source requests.
- No external or custom skill body is canonical for this skill.

## Operating Procedure
1. Confirm the active context is a debug/root-cause Slice with entry gating complete, a named symptom or failing signal, parent Scope or Slice target, and no active live-incident or ops-execution authority override. If the request is not debug/root-cause, route back to variant selection.
2. Build a required-read list from the failure surface: affected source files, nearest tests, known failing commands or logs, runtime/config state, recent diffs and commits, dependency or environment changes, parent Scope constraints, current proof requirements, prior handoff notes, and existing Slice artifacts such as `README.md`, `slice.md`, `symptom.md`, or `reproduction.md`.
3. Run the required context sweeps and record coverage for each:
   - Source sweep: affected code, nearby abstractions, call sites, generated sources, configs, schemas, migrations, and owning docs.
   - Test/proof sweep: nearest tests, failing commands, build/lint/static checks, fixtures, snapshots, and known proof gaps.
   - Log/runtime sweep: captured logs, traces, diagnostics, runtime state, environment/config/dependency versions, and read-only live probes only when already authorized.
   - Recent-change sweep: current git status, branch/worktree target, recent diffs/commits, dependency/config/deploy changes, and uncommitted edits that may affect the failure.
   - Parent/proof sweep: parent Scope constraints, selected Slice variant, artifact contract, proof obligations, authority limits, result boundary, and escalation triggers.
   - Prior-context sweep: README/front-door notes, handoffs, old sessions, memory, issue/MR comments, and stale projections, all marked as hints until verified.
   - Contradiction sweep: disagreements among code, tests, logs, runtime, docs, prior notes, and user statements.
4. Load sources in evidence order, preferring current repo/docs/log artifacts over memory or summary text. Mark each source as current, stale, missing, contradictory, restricted, or out of scope; memory and old session notes are routing hints until verified against current source.
5. Summarize only what the next debug steps need: affected surface, observed failure signal, expected behavior if known, available reproduction handles, relevant files/tests/logs, recent-change window, runtime/config clues, parent constraints, proof gaps, stale context, contradictions, and blocked reads.
6. Update or prepare the `README.md` front-door content only at the context level: current debug target, loaded source list, stale/missing context list, forbidden claims, and next manifest step. Do not write symptom, reproduction, hypotheses, root cause, fix plan, verification, result, promotion, or deployment content from this loader.
7. Emit `current_context_loaded` only when the source list is enough for symptom capture and evidence-order planning. Emit `stale_context_marked` whenever any loaded context is old, memory-derived, projection-derived, missing, contradictory, or environment-dependent.
8. If required context cannot be read, is restricted, or would require execution authority not granted to this step, stop with `request_context_or_block` and name the exact missing source, authority, or refresh needed.

## Outputs
- A debug context packet with these fields: `slice_target`, `entry_gate_status`, `parent_scope_constraints`, `authority_boundary`, `source_refs_loaded`, `source_sweep_coverage`, `test_proof_sweep_coverage`, `log_runtime_sweep_coverage`, `recent_change_window`, `prior_context_checked`, `proof_requirements`, `stale_context`, `missing_context`, `contradictions`, `restricted_pointers`, `blocked_reads`, `gates`, `terminal_state`, and `recommended_next_manifest_step`.
- `README.md` front-door content or update guidance limited to context status: what was loaded, what remains stale or blocked, and which downstream debug step should run next.
- Gate results: `current_context_loaded`, `stale_context_marked` when applicable, and terminal state `context_ready`; otherwise `request_context_or_block` with exact missing evidence.
- No reproduction claim, no evidence-order decision, no hypothesis, no root-cause claim, no fix strategy, no patch, no test writing, no verification claim, no deployment action, no durable-domain write, no promotion, and no completion/result statement.

## Verification
- Check the body has only the seven required `##` sections in order and references the debug-root-cause manifest plus the Part 6B, final-map, and atom-map architecture sources.
- Run `node tools/validate-internal-skill-body-quality.mjs --skill slice-debug-context-loader` and `node tools/validate-internal-skill-trigger-fixtures.mjs --skill slice-debug-context-loader`.
- Inspect trigger fixtures: positive cases must select this skill for debug context loading after entry gating and for stale or missing debug context; negative cases must reject small understood fixes, live incident execution, patching, root-cause declaration, and result writing.
- Review the output packet for concrete source refs, current/stale/missing markings, parent constraints, proof gaps, and no unsupported claims that the bug is reproduced, root-caused, fixed, verified, or complete.

## Failure Modes
- Block as `request_context_or_block` when entry gating is missing, the target Slice or parent Scope is unclear, required files/logs/tests/runtime state cannot be located, or the next step would depend on stale memory.
- Block or hand off when logs, live probes, restricted data, credentials, customer-impacting systems, or deployment state are needed but the active Slice lacks authority for those reads; route the handoff to `slice-debug-handoff-builder` with the missing source, authority owner, and why the loader cannot proceed.
- Route back through variant selection when the work is actually a small known fix, full design/spec work, operational execution, hybrid deploy validation, procedure capture, or durable-domain research.
- Mark contradictions instead of resolving them when code, tests, logs, runtime state, or prior notes disagree; later debug steps own evidence ordering, hypotheses, and root-cause decisions.
- Stop after three failed or contradictory context attempts and request architecture or human clarification rather than widening the loader into diagnosis or fixes; keep the loader at a context-only boundary with no source mutation, workspace mutation, deploy, live command, result, or promotion side effect.
