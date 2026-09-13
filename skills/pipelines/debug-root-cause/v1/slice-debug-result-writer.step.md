---
id: "slice-debug-result-writer"
kind: "internal_step"
owner_family: "slice_variant"
pipeline_id: "slice.debug-root-cause"
step_id: "slice-debug-result-writer"
entry_gate: false
public_trigger: false
native_skill_discovery: false
body_path: "capabilities/instructions/pipelines/slice-variants/debug-root-cause/slice-debug-result-writer.step.md"
source_manifest: "capabilities/pipelines/slice-variants/debug-root-cause.pipeline.json"
legacy_skill_ref: "slice-debug-result-writer"
architecture_refs:
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#debug-root-cause-variant"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19"
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#lightweight-and-debug-slice-variants"
---

# Debug Result Writer

## Overview
This skill closes the Debug/root-cause Slice by writing `result.md` from the highest validated truth already proven by the diagnostic and verification steps. Its core rule is no upgraded truth: the result may summarize root cause, fix/no-fix, and proof, but it cannot invent proof, run more diagnosis, or call the work complete beyond the evidence.

## When to Use
Use this when the active pipeline is `slice.debug-root-cause`, required debug artifacts exist or their absence is explicitly recorded, and the next step is final result recording. Typical inputs are `symptom.md`, `reproduction.md` or an unable-to-reproduce record, `evidence-log.md`, `hypotheses.md`, `root-cause.md`, `fix-plan.md` or `no-fix-result.md`, `verification.md`, and any optional `patch.md`, diagnostics, logs, traces, screenshots, deferred notes, or handoff constraints.

Do not use this to capture symptoms, build reproductions, gather evidence, choose root cause, plan or apply fixes, run verification, route promotion, or prepare a blocked handoff. Route backward when proof is missing, stale, contradictory, or still being collected.

The trigger is exact: select this skill only when the debug Slice has reached the manifest result step and the remaining work is to write the result artifact from existing proof. The scope boundary is equally exact: read Slice artifacts, classify truth, write `result.md`, and stop; any need to mutate source, run commands, collect proof, deploy, promote, or repair maintenance state belongs to another step.

## Source Contract
Grounding sources:
- `capabilities/pipelines/slice-variants/debug-root-cause.pipeline.json` step `slice-debug-result-writer`
- `capabilities/pipelines/slice-variants/debug-root-cause.pipeline.json#step_graph.steps.slice-debug-result-writer.invokes.slice-debug-result-writer`
- `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#debug-root-cause-variant`
- `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#lightweight-and-debug-slice-variants`
- `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6`
- `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19`
- `docs/architecture/master-plugin-target-architecture-capability-atom-shard-map.html#pipeline.slice.debug_root_cause.result.writer`

The manifest produces `result.md`, gates on `highest_truth_recorded`, fails with `block_missing_result`, and reaches `result_ready`. No external reference body is required for this skill.

The result field for closure classification is `highest_validated_truth`. It must be derived from the selected debug artifacts and proof packet, not from desired outcome, user pressure, or the fact that a fix plan or patch exists.

## Operating Procedure
1. Load the debug Slice contract and verification packet. Confirm the selected variant is `slice.debug-root-cause`, then list which required debug artifacts are present, absent, superseded, or intentionally replaced by an unable-to-reproduce or no-fix record.
2. Classify `highest_validated_truth`. Choose the strongest state the evidence supports: root cause found and locally verified, no-fix result verified, unable to reproduce with proof, fix deferred, blocked missing proof, blocked missing authority, escalated to full development, escalated to operation, or superseded by follow-up work.
3. Audit proof freshness and scope. Compare every completion, fixed, deployed, live, safe, no-fix, or unable-to-reproduce claim against `verification.md`, reproduction evidence, regression proof, affected tests, runtime checks, and any explicit handoff or authority record. Downgrade claims that exceed the evidence.
4. Record the result body. Include symptom summary, root cause or unknown-cause status, rejected hypotheses when relevant, fix or no-fix decision, proof commands or evidence references, proof class, `highest_validated_truth`, residual risk, missing proof, forbidden claims, deferred work, promotion candidate status, and the next action.
5. Set terminal state. Use `result_ready` only when `result.md` states the highest truth and preserves every blocker, residual risk, and forbidden claim. If the result cannot be written honestly, stop with `block_missing_result` and name the exact missing artifact, proof, authority, or decision.
6. Preserve boundaries. This skill may format the result artifact and final handoff text, but it does not authorize source mutation, test execution, deployment, live-system commands, durable-domain writes, promotion, branch operations, or maintenance repair.

## Outputs
Produce `result.md` for the selected debug Slice. The artifact must include the debug outcome, `highest_validated_truth`, terminal state, evidence summary, proof freshness, root-cause or unknown-cause status, fix/no-fix result, missing proof, residual risk, forbidden claims, deferred work, promotion candidate status, and handoff or next action.

Use these terminal states at this step: `result_ready` when the artifact truth is recorded, or `block_missing_result` when the truthful artifact cannot be written. Preserve upstream debug states inside the body when relevant, including `root_cause_found`, `unable_to_reproduce`, `blocked_missing_evidence`, `fix_deferred_to_followup`, `blocked_missing_proof`, `handoff_required`, or `completed_local_verified`; do not substitute them for the manifest terminal state.

The only successful manifest terminal state for this step is `result_ready`. When the evidence cannot support a truthful result, emit `block_missing_result` with the missing proof or authority instead of producing a completion-flavored result.

## Verification
Trigger verification requires positive scenarios where a debug/root-cause Slice is ready to record final result truth after root-cause and verification work, plus negative scenarios where diagnosis, fix execution, proof collection, promotion, or handoff is still the next step.

Content verification requires the body to preserve the manifest path, architecture HTML sources, `result.md`, `highest_truth_recorded`, `block_missing_result`, and `result_ready`; to include concrete result-writing procedure; and to avoid wrapper sections, manifest dumps as behavior, execution authority, and any claim that local proof implies deployment or live truth.

Closure verification is a field-by-field proof audit: every allowed claim in `result.md` must have a cited artifact or command in the Slice evidence, every missing proof must remain visible, and every forbidden claim must stay forbidden until the owning pipeline records fresh proof. The `highest_validated_truth` value is valid only when it is no stronger than the proof class recorded in `verification.md` or the explicit unable-to-reproduce/no-fix record.

Run:
- `node tools/validate-internal-skill-body-quality.mjs --skill slice-debug-result-writer`
- `node tools/validate-internal-skill-trigger-fixtures.mjs --skill slice-debug-result-writer`

## Failure Modes
Block with `block_missing_result` when required artifacts are absent without explanation, reproduction or unable-to-reproduce truth is missing, root cause is unproven but presented as proven, verification is stale, proof skips the named cause, no-fix reasoning is thin, authority is missing, or the requested result would upgrade local proof into deploy or live proof.

Route backward to reproduction, evidence, root-cause, fix strategy, regression, or verification steps when more debug work is needed. Route sideways to operational execution, hybrid implementation, full development, architecture discussion, or handoff when the debug Slice no longer owns the next action. Route forward to promotion only after `result.md` already records highest validated truth and any reusable learning candidate without writing to a durable domain.

If a user asks for a short final answer while the Slice evidence is incomplete, keep the result blocked and provide a handoff summary rather than softening the terminal state. If artifacts disagree, preserve the contradiction in `result.md` and route to the earliest step that can repair source truth. Treat that as zero-proof closure pressure, not as permission to complete.

Forbidden actions are source mutation, test execution, deployment, live-system command, durable-domain write, promotion, branch operation, maintenance repair, proof fabrication, and upgrading local proof to deployed or live truth. This skill can name those as required next actions, but it cannot perform or authorize them.
