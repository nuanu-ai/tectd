---
id: "slice-hypothesis-ledger"
kind: "internal_step"
owner_family: "slice_variant"
pipeline_id: "slice.debug-root-cause"
step_id: "slice-hypothesis-ledger"
entry_gate: false
public_trigger: false
native_skill_discovery: false
body_path: "capabilities/instructions/pipelines/slice-variants/debug-root-cause/slice-hypothesis-ledger.step.md"
source_manifest: "capabilities/pipelines/slice-variants/debug-root-cause.pipeline.json"
legacy_skill_ref: "slice-hypothesis-ledger"
architecture_refs:
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#debug-root-cause-variant"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19"
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#lightweight-and-debug-slice-variants"
---

# Slice Hypothesis Ledger

## Overview

This is a reference adapter skill for `superpowers:systematic-debugging`, narrowed to Tect's debug/root-cause Slice. Its core rule is: `hypotheses.md` is the visible scientific record between evidence gathering and root-cause decision. No hidden guess, confidence jump, or fix attempt can pass this step.

## When to Use

Use this when a debug/root-cause Slice has a symptom, reproduction or unable-to-reproduce record, evidence log, and one or more possible explanations that are not yet proven. It also applies when evidence contradicts the current candidate, a rejected guess needs preservation, confidence is rising without new proof, or a worker wants to "just try" a fix.

Do not use this for initial symptom capture, reproduction building, broad evidence ordering, final root-cause declaration, fix strategy, regression-test writing, source edits, deployment, live-system checks, or result writing. If the ledger already proves one cause over alternatives, hand off to root-cause decision instead of reopening diagnosis.

## Source Contract

Grounding:

- `capabilities/pipelines/slice-variants/debug-root-cause.pipeline.json#step_graph.steps.slice-hypothesis-ledger`
- `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#lightweight-and-debug-slice-variants`
- `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#debug-root-cause-variant`
- `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6`
- `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19`
- `docs/architecture/master-plugin-target-architecture-capability-atom-shard-map.html#pipeline.slice.debug_root_cause.hypothesis.ledger`
- `docs/architecture/master-plugin-target-architecture-validation-harness-evals.html`

Manifest contract: produce `hypotheses.md`, satisfy gate `hypotheses_and_rejections_recorded`, produce terminal state `hypotheses_ready`, and block with `block_untracked_guess`.

Reference source: `skills/references/superpowers/systematic-debugging/SKILL.md`. Adapted behaviors are root cause before fix, reproduce before deciding, gather evidence before action, form a single hypothesis, test minimally, reject contradicted guesses, and stop rather than stacking another fix.

## Operating Procedure

1. Gate the inputs. Require an active debug/root-cause Slice, `symptom.md`, `reproduction.md` or an unable-to-reproduce note, `evidence-log.md`, and an authority boundary. If the next action is an implied cause or fix, write the ledger first.
2. Generate hypotheses from evidence. This skill generates hypotheses from evidence by converting logs, tests, diffs, runtime state, recent changes, working examples, and data-flow traces into specific candidate root causes. Each candidate must name a mechanism, not a vague bucket such as "config issue."
3. Record each hypothesis. For every candidate, `hypotheses.md` records candidate root causes, predicted observations, evidence for, evidence against, tests/evidence already run, next discriminating test, next evidence needed, status, confidence, owner, and timestamp or source reference.
4. Test one hypothesis at a time. Keep only one `active` hypothesis. The next check must say what observation would strengthen it, what observation would weaken or reject it, and which competing explanation it discriminates from. Park other candidates as `candidate` or `inactive`; never run parallel fix attempts.
5. Reject guesses without deleting them. `hypotheses.md` records rejected guesses and why they were rejected, including the exact contradictory evidence, rejection reason, whether the rejection is final or conditional, and what future evidence would reopen it.
6. Track confidence without declaring root cause too early. Confidence may increase only after new evidence matches predicted observations and competing hypotheses are weakened. A high-confidence candidate is still not a root cause until `slice-root-cause-decision` accepts it.
7. Handle contradictions before action. This skill handles contradictions by pausing and updating the ledger before new action: mark the contradiction, lower or freeze confidence, add the smallest clarifying evidence check, and block any fix path until the conflict is resolved or explicitly carried as residual uncertainty.
8. Prevent confirmation bias. For each active hypothesis, require at least one disconfirming observation or test. Do not choose checks merely because they are likely to confirm the favored story; the next evidence must be able to change the ledger.
9. Prevent untracked guesses and fix attempts. This skill prevents untracked guesses by forcing every causal claim into `hypotheses.md`; it prevents fix attempts before proof by routing any patch, config change, deploy, live mutation, or implementation action to `block_untracked_guess`.
10. Exit only when the ledger is complete. The skill produces terminal state `hypotheses_ready` when every active, candidate, inactive, or rejected hypothesis has evidence for and against, confidence, status, next evidence or rejection reason, and no hidden guess remains. Then hand off to root-cause decision, not fix execution.

## Outputs

Maintain `hypotheses.md` for the active debug/root-cause Slice. It must include candidate root causes, evidence for and evidence against each candidate, predicted observations, tests/evidence performed, confidence, status, next discriminating checks, next evidence needed, contradiction notes, rejected guesses, rejection reason, and the next allowed handoff.

Allowed hypothesis statuses are `candidate`, `active`, `testing`, `weakened`, `rejected`, `blocked`, and `ready_for_decision`. The terminal state is `hypotheses_ready`, which only hands off to `slice-root-cause-decision` or a named evidence-gathering step. Failed output blocks with `block_untracked_guess` and names the missing ledger field, untracked causal claim, contradiction, confirmation-bias risk, or forbidden fix attempt.

This skill does not produce `root-cause.md`, `fix-plan.md`, `verification.md`, or `result.md`.

## Verification

Verify trigger fit by checking that the selected work is a debug/root-cause Slice and the immediate need is hypothesis tracking, not reproduction, broad evidence planning, root-cause approval, implementation, or result writing. Verify content by reading `hypotheses.md` and confirming each candidate has a specific root cause statement, predicted observations, evidence for, evidence against, confidence, status, discriminating check, next evidence field, and rejection reason when rejected.

Static validation should pass:

- `node tools/validate-internal-skill-body-quality.mjs --skill slice-hypothesis-ledger`
- `node tools/validate-internal-skill-trigger-fixtures.mjs --skill slice-hypothesis-ledger`

Manual review should reject any ledger that lets a fix begin before proof, hides contradicted evidence, deletes rejected guesses, keeps several guesses active at once, lacks disconfirming checks, upgrades confidence without new evidence, or claims root cause before the decision gate.

## Failure Modes

Block with `block_untracked_guess` when a hypothesis is implied in discussion but absent from `hypotheses.md`, when a proposed fix has no proven root cause, when evidence for or against a candidate is missing, when predicted observations are absent, when confidence changes without evidence, when rejected guesses lose their rejection reason, or when the next check does not discriminate between alternatives.

Route back to reproduction or evidence planning when the symptom is vague, the reproduction is absent, or the evidence log is too thin to form a testable hypothesis. Route to data-flow tracing when the current hypothesis depends on an unknown value or state transition. Route to root-cause decision only when the ledger already supports one candidate over alternatives.

This skill has no fix, source edit, deployment, live-system command, branch, durable write, result-writing, or promotion authority. If the next useful evidence check needs that authority, record the needed authority, status `blocked`, and handoff target rather than executing it.

## Quick Reference

Records candidate root causes. Records rejected guesses and why they were rejected. Tests one hypothesis at a time. Prevents untracked guesses. Prevents fix attempts before proof. Preserves rejected guesses. Blocks confirmation bias. Produces terminal state `hypotheses_ready` or blocks with `block_untracked_guess`.
