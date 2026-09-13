---
id: "slice-working-example-comparator"
kind: "internal_step"
owner_family: "slice_variant"
pipeline_id: "slice.debug-root-cause"
step_id: "slice-working-example-comparator"
entry_gate: false
public_trigger: false
native_skill_discovery: false
body_path: "capabilities/instructions/pipelines/slice-variants/debug-root-cause/slice-working-example-comparator.step.md"
source_manifest: "capabilities/pipelines/slice-variants/debug-root-cause.pipeline.json"
legacy_skill_ref: "slice-working-example-comparator"
architecture_refs:
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#debug-root-cause-variant"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19"
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#lightweight-and-debug-slice-variants"
---

# Slice Working Example Comparator

## Overview

This skill compares similar working paths against the broken path during `slice.debug-root-cause`. Core rule: use a working contrast to narrow evidence and hypotheses, not to guess a fix or declare root cause.

## When to Use

Use this after the Slice has a captured symptom and some reproduction, evidence log, trace, recent-change record, runtime observation, or test output showing the broken path. It fits when there is a comparable healthy flow, older good revision, passing test, unaffected environment, alternate tenant, similar component, known-good config, or documented expected path that can be inspected safely.

Do not use this to create the first reproduction, run broad recent-change inspection, trace raw data flow, maintain the hypothesis ledger, declare root cause, choose or apply a fix, write regression tests, verify a fix, or handle live incident execution. If no comparable working example can be located with available evidence, record that explicitly and route forward with the comparison unavailable.

## Source Contract

Grounding:

- `capabilities/pipelines/slice-variants/debug-root-cause.pipeline.json#step_graph.steps.slice-working-example-comparator`
- `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#debug-root-cause-variant`
- `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#lightweight-and-debug-slice-variants`
- `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6`
- `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19`
- `docs/architecture/master-plugin-target-architecture-capability-atom-shard-map.html#pipeline.slice.debug_root_cause.working.example.comparator`

Manifest contract: this optional debug step produces the step-owned `working-example.md` receipt and may update `evidence-log.md`, supports `working_example_compared_or_unavailable`, returns `comparison_ready` when comparison or unavailable state is recorded, and records `record_missing_comparison` when a needed comparison cannot be made. A future-check reference in the evidence plan cannot satisfy this step.

## Operating Procedure

1. Gate the inputs. Require an active debug/root-cause Slice, a broken-path symptom reference, and at least one evidence source that shows the failure. If the symptom or reproduction boundary is absent, route to earlier debug steps.
2. Identify candidate working examples. Prefer the closest safe comparator: same component with passing input, previous known-good revision, unaffected environment, similar endpoint, alternate config, passing test fixture, or documented expected flow. Record why each candidate is comparable, what dimension differs, and which candidate is the primary comparator.
3. Define the comparison dimensions before inspecting details. Include inputs, preconditions, environment/config, data shape, control path, dependency versions, state transitions, permissions, timing, external service behavior, and output or error surface as relevant.
4. Compare facts side by side. For each dimension, cite the evidence for the broken path and the evidence for the working path. Mark each difference as confirmed, absent, unknown, or unsafe to inspect. Do not promote correlation to cause.
5. Extract discriminating evidence. Name the smallest next check that would distinguish whether a confirmed difference explains the symptom. If that check needs mutation, credentials, deployment, live-system action, or operator authority, record the authority requirement and stop.
6. Write `working-example.md` with the comparison receipt: broken path, comparator, matched dimensions, meaningful differences, shared invariants, unknowns, blocked checks, hypothesis impact, evidence refs, and next route. If no working example is available, record the search scope, rejected candidates, access limits, and reason comparison is unavailable. Optionally append a concise summary to `evidence-log.md`.
7. Write the manifest gate state explicitly in `working-example.md`. A successful receipt contains exactly one of `comparison_recorded` or `comparison_unavailable`, plus `gate: working_example_compared_or_unavailable` and `terminal_state: comparison_ready`. A failed receipt records `record_missing_comparison` and cannot claim that success pair.
8. Exit only with a comparison-ready handoff. Route confirmed differences to `slice-hypothesis-ledger`, data gaps to tracing or evidence planning, and sufficient causal proof to root-cause decision. Do not enter fix work from this step.

## Outputs

Produce `working-example.md`. It must include the broken path, comparator path, comparability rationale, comparison dimensions, evidence references for both sides, confirmed differences, shared invariants, unknowns, unsafe or blocked checks, and the next suggested diagnostic route. A concise summary may also be appended to `evidence-log.md`, but that shared plan/log is not the completion carrier.

Successful output reaches `comparison_ready` and satisfies `working_example_compared_or_unavailable` by either completing the comparison or explicitly recording why a comparator is unavailable. The status block must name exactly one of `comparison_recorded` or `comparison_unavailable`, the exact gate and terminal-state markers, the evidence path, and the next diagnostic route. Failed output records `record_missing_comparison` without the success markers, with the missing source, owner, access, or evidence needed. This skill does not produce `root-cause.md`, `fix-plan.md`, `verification.md`, `result.md`, or source patches.

## Verification

Verify trigger fit by confirming the work is already in the debug/root-cause variant and the immediate need is contrastive evidence from a working path, not reproduction, broad tracing, final root-cause approval, or fix execution. Verify content by reading `evidence-log.md` and checking that both sides of the comparison cite evidence, the dimensions are explicit, and unavailable comparison state is documented when no comparator exists.

Static validation should pass:

- `node tools/validate-internal-skill-body-quality.mjs --skill slice-working-example-comparator`
- `node tools/validate-internal-skill-trigger-fixtures.mjs --skill slice-working-example-comparator`

Manual review should reject comparisons that rely on memory, omit the broken or working side, hide unknowns, convert a difference directly into root cause, or authorize mutation beyond read-only inspection.

## Failure Modes

Block with `record_missing_comparison` when the comparator cannot be located, the available exemplar is not meaningfully comparable, evidence for either side is missing, the next check requires unapproved authority, or inspecting the working example would mutate source, runtime state, deployment, data, packages, branches, or live systems.

Route back to reproduction when the broken path is not reliable enough to compare. Route to data-flow tracing when the difference is inside an unknown value or state transition. Route to the hypothesis ledger when the comparison creates or weakens candidate causes. Route to root-cause decision only when comparison evidence is already strong enough and alternate explanations are handled elsewhere.

If the comparison produces only weak similarity, record that as low-confidence evidence instead of stretching it into a causal story. If the working path is available only through privileged data, destructive replay, package installation, branch mutation, deployment, or live-system commands, preserve the blocked request and owner for handoff.

Never use this skill to patch code, run a fix, declare completion, perform deployment or live-system commands, write result status, promote knowledge, or replace root-cause proof with a familiar-looking working example. Use a zero-authority handoff when the comparison needs anything beyond safe inspection.
