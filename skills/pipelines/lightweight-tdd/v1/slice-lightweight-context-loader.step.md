---
id: "slice-lightweight-context-loader"
kind: "internal_step"
owner_family: "slice_variant"
pipeline_id: "slice.lightweight-tdd-development"
step_id: "slice-lightweight-context-loader"
entry_gate: false
public_trigger: false
native_skill_discovery: false
body_path: "capabilities/instructions/pipelines/slice-variants/lightweight-tdd/slice-lightweight-context-loader.step.md"
source_manifest: "capabilities/pipelines/slice-variants/lightweight-tdd.pipeline.json"
legacy_skill_ref: "slice-lightweight-context-loader"
architecture_refs:
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#lightweight-development"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19"
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#lightweight-and-debug-slice-variants"
---

# Slice Lightweight Context Loader

## Overview
This skill loads the smallest current context packet needed for a Lightweight TDD Slice. Its core rule is bounded freshness with zero action authority: read immediate truth, preserve Slice identity and parent constraints, expose proof and escalation needs, then stop before test selection, source mutation, verification, deployment, result writing, or promotion.

## When to Use
Use this after lightweight entry and intent capture have selected `slice.lightweight-tdd-development`, the request is a small understood code/config/business change, acceptance behavior is clear, and downstream lightweight steps need current files, docs, tests, parent Scope constraints, proof needs, git/worktree cleanliness signals, or deferred notes.

Use it when the next decision depends on what context is safe to trust, which source surfaces are relevant, what parent artifact contract applies, whether local proof can stay bounded, or whether earlier notes are stale, missing, restricted, or contradicted by current source.

Do not use it for unknown-cause bugs, architecture ambiguity, cross-component design, operation execution, code changes, test execution, deployment/live validation, result writing, promotion, current-state-only lookup, or broad research. Route those to debug, full-design, hybrid/ops, result, promotion, query, or research steps as appropriate.

## Source Contract
Ground this behavior in `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#lightweight-development`, `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#lightweight-and-debug-slice-variants`, `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6`, `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19`, and `docs/architecture/master-plugin-target-architecture-capability-atom-shard-map.html`.

The owning manifest is `capabilities/pipelines/slice-variants/lightweight-tdd.pipeline.json`. Its `slice-lightweight-context-loader` step is required, invokes this skill, contributes `README.md`, gates on `bounded_context_loaded`, reaches `context_ready`, and fails through `request_context_or_escalate_full`.

The relevant atom row is `pipeline.slice.lightweight_tdd.context.loader`: load immediate files/docs/tests plus parent constraints and relevant proof/deferred notes for a small understood change. Neighboring lightweight rows require preserving escalation, workspace preflight, deploy impact, test target, TDD runner, verification, result, and promotion boundaries. No external skill body is a source for this skill.

## Operating Procedure
1. Confirm the active Slice identity. Require a selected lightweight variant, parent Program/Scope/Slice pointers or an explicit absence note, clear acceptance checks, and the parent artifact contract for the selected Slice folder. If identity or parentage is ambiguous, stop with `request_context_or_escalate_full`.
2. Recheck scope before loading. The request must stay small, understood, local-proof-oriented, and low consequence. If the user introduces unknown root cause, architecture uncertainty, multi-component design, deploy/live dependency, team authority, or operational execution, route to debug, full, hybrid, or ops before reading more.
3. Build the minimum read list. Sweep only: user-named files, nearest source/config/package files, colocated or obvious tests as proof candidates, local project instructions, relevant docs/README, parent Scope constraints, current Slice notes, prior proof/deferred records, restricted-source pointers, and existing git/worktree cleanliness or branch-provenance signals supplied by Runtime/preflight artifacts.
4. Load source truth read-only. Prefer current files, manifest/runtime fields, parent artifacts, and existing proof records over memory. For each fact, record path or field, source class, freshness, and status: current, historical, derived, stale, missing, contradicted, or restricted.
5. Apply authority and proof gates. State the source mutation policy as `no source mutation`, preserve that this loader has no command/test/deploy authority, and mark whether local proof still appears possible. If deployment proof, live proof, protected-branch action, worktree isolation, or approval is implicated, do not resolve it here; hand the signal to deploy-impact, workspace-preflight, escalation, hybrid, or ops routing.
6. Shape the context packet. Include only Slice identity, parent contract, acceptance boundary, loaded source surfaces, relevant tests as candidates, proof/freshness needs, authority limits, git/worktree cleanliness signals, stale/missing/restricted items, escalation triggers, and the recommended next step. Exclude broad history, speculative causes, implementation ideas, selected test commands, result claims, or durable promotion text.
7. Decide the gate. If the packet is sufficient for `slice-lightweight-contract-writer`, `slice-workspace-preflight-lite`, and `slice-test-target-selector` to continue, write the exact lines `gate: bounded_context_loaded` and `terminal_state: context_ready`. If not, write `gate: request_context_or_escalate_full` and `terminal_state: request_context_or_escalate_full` with the exact missing source, stale authority, restricted access, or routing reason. A failure packet must never contain either success line.
8. Stop at context. This skill does not perform test-target selection, TDD execution, source edits, command execution, deployment, live validation, no result claim, no promotion, branch/worktree mutation, cleanup, or active pipeline-state persistence.

## Outputs
Return a compact `README.md`-shaped context packet for the active Lightweight TDD Slice.

The packet must contain: Slice identity and selected variant, parent Program/Scope/Slice pointers, parent artifact contract, acceptance boundary, loaded source list with freshness labels, immediate files/docs/tests, proof candidates without selecting commands, source mutation policy, git/worktree cleanliness or missing-preflight signal, deploy/live validation implication if any, stale/missing/restricted facts, escalation triggers, and the recommended next step.

The successful packet must contain the exact machine-readable lines `gate: bounded_context_loaded` and `terminal_state: context_ready`. The failed terminal path is `request_context_or_escalate_full`, including the exact missing context, authority gap, freshness conflict, or escalation route, and it must omit both success lines.

## Verification
Verify the trigger by checking that the scenario is a small clear Lightweight TDD Slice needing bounded context, not diagnosis, design, ops, implementation, tests, deployment, result, promotion, or query-only work. Verify the content by confirming every claim in the packet is tied to a current path, manifest/runtime field, parent constraint, existing proof artifact, or explicitly labeled stale/missing/restricted source.

Check the authority boundary explicitly: no source mutation, no git/worktree mutation, no tests, no deployment, no live validation, no result claim, and no promotion. Check that git/worktree cleanliness, proof needs, source authority, and freshness are either loaded from existing evidence or marked missing for the next step.

Run the Layer 6B checks for this skill: `node tools/validate-internal-skill-body-quality.mjs --skill slice-lightweight-context-loader` and `node tools/validate-internal-skill-trigger-fixtures.mjs --skill slice-lightweight-context-loader`. The body must retain the manifest path, at least one architecture HTML path, the exact gate and terminal states, and the seven required H2 sections in order.

## Failure Modes
Block or escalate when acceptance is unclear, Slice identity is missing, parent constraints cannot be found, required files/tests/docs are inaccessible, source authority is restricted, current reads contradict prior notes, git/worktree cleanliness is unknown and material, the target spans multiple components without a parent Scope decision, or the user reports unexpected behavior with unknown cause.

Escalate out of lightweight when missing tests change the proof path, repeated failures are already present, implementation would need protected-branch/worktree authority, deploy/live proof is needed for completion, or local proof would be misleading. Route debug symptoms to debug/root-cause, broad design to full design-to-execution, deploy/live coupling to hybrid or ops, and current-state lookup to query services.

If the user asks for implementation, verification, deployment, result, or promotion while context is still missing, hand off with the exact gap stated. If the request is only a current-state query or artifact lookup, do not select this skill; route to query/context services instead. Keep this as a zero-action context boundary so lightweight context loading never becomes hidden full workflow execution.
