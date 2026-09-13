---
id: "slice-debug-handoff-builder"
kind: "internal_step"
owner_family: "slice_variant"
pipeline_id: "slice.debug-root-cause"
step_id: "slice-debug-handoff-builder"
entry_gate: false
public_trigger: false
native_skill_discovery: false
body_path: "capabilities/instructions/pipelines/slice-variants/debug-root-cause/slice-debug-handoff-builder.step.md"
source_manifest: "capabilities/pipelines/slice-variants/debug-root-cause.pipeline.json"
legacy_skill_ref: "slice-debug-handoff-builder"
architecture_refs:
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#lightweight-and-debug-slice-variants"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19"
---

# Slice Debug Handoff Builder

## Overview
This skill produces the `handoff.md` packet for a Debug Root-Cause Slice when work must pause, transfer, or resume later. The core rule is continuity without invention: preserve source-backed debug state and blockers, but do not create proof, decide root cause, apply fixes, verify, promote, deploy, or close unresolved diagnostic work.

## When to Use
Use this only inside or directly beside a selected `slice.debug-root-cause` lifecycle when a concrete Slice must be handed to a future agent, the user, a team, an architecture discussion, or an operations owner because one of these is true:

- Evidence is missing or stale.
- Authority is absent.
- Environment, logs, runtime, or live access is unavailable.
- Context budget is ending and another actor must resume.
- An architecture decision is needed, including after three failed fix attempts.
- A human/team action must occur before the next debug step.

Use it only when the packet can name the Debug Slice and can point to source-backed debug artifacts or explicit missing-artifact records. Do not use it for first symptom capture, reproduction building, recent-change inspection, evidence ordering, data-flow tracing, hypothesis updates, root-cause decisions, fix strategy, regression test creation, patch application, verification, result writing, promotion routing, operational execution, deployment, or final completion claims.

## Source Contract
Bind the packet to these sources:

- `capabilities/pipelines/slice-variants/debug-root-cause.pipeline.json`: step `slice-debug-handoff-builder`, type `handoff`, optional, invokes this skill, produces `handoff.md`, gate `handoff_complete_when_needed`, failure `block_missing_handoff`, terminal states `handoff_required` and `completed_local_verified`.
- `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#lightweight-and-debug-slice-variants`: Debug/root-cause starts when observed behavior conflicts with expected behavior and root cause is unknown. It requires symptom, reproduction or unable-to-reproduce truth, evidence, hypotheses, root cause, fix or no-fix result, verification, and result before fixed/done.
- `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6`: Slice completion needs proof or explicit handoff/blocker; local proof is not live proof.
- `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19`: runtime chooses debug for bug/regression unknown-cause work and closes through result, promotion, deferred, handoff, no-promote, or blocked state.
- `docs/architecture/master-plugin-target-architecture-capability-atom-shard-map.html` row `pipeline.slice.debug_root_cause.handoff.builder`: write handoff when blocked by missing evidence, authority, environment, live access, or architecture decision.

Read these source inputs before writing:

- Parent Slice identity and variant source: `README.md`, `slice.md`, runtime view, or manifest-backed Slice record.
- Required debug artifacts: `symptom.md`, `reproduction.md`, `evidence-log.md`, `hypotheses.md`, `root-cause.md`, `fix-plan.md` or `no-fix-result.md`, `verification.md`, and `result.md`.
- Optional debug artifacts: `git-history.md`, `timeline.md`, `diagnostics/`, `logs/`, `traces/`, `screenshots/`, `patch.md`, `promotion.md`, `deferred.md`, and prior `handoff.md`.
- Authority and access facts: who can approve, what environment or live access is missing, and what source cannot be inspected now.
- External reference bodies such as systematic debugging, TDD, planning, verification, subagent, executing-plans, writing-plans, using-superpowers, writing-skills, and skill-creator are reference insight only. Do not make them runtime dependencies or delegate this step to them.

## Operating Procedure
1. Confirm scope. Require a concrete Debug Root-Cause Slice identity, selected variant, and target actor. If the work is not a debug handoff, route away before writing.
2. Identify the handoff trigger. Use one or more: `missing evidence`, `missing authority`, `environment unavailable`, `live access unavailable`, `context overflow`, `human/team action`, `architecture decision`, `future-agent continuation`, `follow-up Slice`, or `operation escalation`.
3. Sweep artifacts. For every required and optional source input, record status as `present`, `missing`, `stale`, `superseded`, or `not applicable`; include path, timestamp/version if known, and one-line consequence.
4. Sweep debug state. Preserve exact symptom, expected/observed delta, reproduction status, evidence order, hypotheses, rejected hypotheses, current root-cause claim or unknown state, failed fix count, fix/no-fix choice, verification status, and highest truth already written by result/promotion artifacts.
5. Sweep proof. Classify each claim as `directly verified`, `inferred from evidence`, `unverified`, `disproven`, `blocked by access`, or `stale`. Do not upgrade missing reproduction, unknown root cause, skipped verification, or stale live behavior into proof.
6. Build `handoff.md` using the artifact shape below. Fill unknowns as explicit blockers; never smooth over them with narrative confidence.
7. Choose a terminal state without performing the next action. Use `handoff_required` when action is blocked, delegated, or awaiting authority. Use `completed_local_verified` only when existing `verification.md` and `result.md` already prove local completion and the packet is continuity packaging.
8. Route the next owner. Gather more evidence, ask user/team for authority or access, continue the debug pipeline at the next named step, create a lightweight/full follow-up Slice, escalate operational recovery to operational execution, request architecture discussion after repeated failed fixes or redesign need, or hand proven result/promotion work to the result or promotion step.
9. Stop if the packet would depend on chat memory, undocumented claims, missing parent Slice identity, hidden artifact paths, source mutation, live commands, or unapproved deployment.

`handoff.md` shape:

```markdown
# Debug Handoff: <slice id or title>

## Packet Metadata
- Variant: slice.debug-root-cause
- Manifest step: slice-debug-handoff-builder
- Gate: handoff_complete_when_needed
- Terminal state: handoff_required | completed_local_verified
- Target actor:
- Handoff reason:
- Current step:
- Last completed step:

## Preserved Debug State
- Symptom:
- Expected vs observed:
- Reproduction status:
- Evidence order:
- Hypotheses and rejected hypotheses:
- Root-cause state:
- Failed fix count:
- Fix/no-fix state:
- Verification state:
- Result/highest validated truth:

## Artifact Inventory
| Artifact | Status | Source path or absence | Freshness | Consequence |
| --- | --- | --- | --- | --- |

## Proof Inventory
| Claim | Proof class | Source | Status | Missing proof |
| --- | --- | --- | --- | --- |

## Blockers And Needs
- Missing evidence:
- Authority needed:
- Environment or live access needed:
- Architecture decision needed:

## Resume Contract
- Exact next action:
- Safe resume trigger:
- Forbidden claims:
- Reroute options:
```

## Outputs
- `handoff.md` for the current Debug Root-Cause Slice with packet metadata, preserved debug state, artifact inventory, proof inventory, blockers, authority/environment/live-access needs, exact next action, safe resume trigger, forbidden claims, and reroute options.
- A terminal-state recommendation: `handoff_required` or `completed_local_verified`, with the source artifact that justifies it.
- A route recommendation when needed: gather more evidence, ask user/team for authority or access, continue at a named debug step, create a follow-up lightweight/full Slice, escalate to operational execution, request architecture discussion, or pass already-proven closure work to Result / Promotion.
- A request for a transition record or follow-up Slice proposal only when the owning pipeline step must persist it. This skill itself does not persist active state, mutate source, deploy, promote, or close the lifecycle.

## Handoff Routing
- Missing proof, missing access, missing authority, context overflow, or future-agent continuation: `handoff_required`.
- Existing local verification and result already prove closure, but another actor needs context: `completed_local_verified`.
- Root cause unknown or reproduction absent without an unable-to-reproduce record: route back to debug evidence/reproduction/root-cause steps.
- Code change requested: route to debug fix runner only when root cause and regression proof are already established; otherwise route to lightweight/full Slice follow-up.
- Production recovery, live user impact, deploy authority, or rollback work dominates: route to operational execution or hybrid implementation/operation.
- Three or more failed fixes, redesign need, or cross-component architecture uncertainty: route to architecture discussion before another fix.
- Reusable debugging procedure, stale KB, runbook, or follow-up learning: route to promotion/router skills after result proof exists.

## Forbidden Actions
- No source mutation or deployment authority: do not edit application/source files, branches, worktrees, registries, ledgers, runtime state, live systems, deployment targets, or durable-domain knowledge.
- Do not run fixes, live commands, deploys, destructive commands, promotion writes, maintenance repairs, or result finalization.
- Do not create a new root-cause decision, fix plan, verification result, live-proof claim, promotion decision, or completion claim.
- Do not hide rejected hypotheses, failed fixes, stale proof, or missing evidence.
- Do not silently delegate to external skills. Reference their discipline concepts only when they already match the Tect source contract.

## Verification
Before treating `handoff.md` as usable, verify all gates:

- Manifest gate: packet references `capabilities/pipelines/slice-variants/debug-root-cause.pipeline.json`, step `slice-debug-handoff-builder`, gate `handoff_complete_when_needed`, and one allowed terminal state.
- Slice gate: packet names the concrete Debug Root-Cause Slice, selected variant, current step, last completed step, target actor, and handoff trigger.
- Artifact gate: every required artifact is either listed with a source path or explicitly marked missing/stale/superseded/not applicable with consequence.
- Proof gate: every factual statement is tied to an artifact, command output, log, trace, screenshot, result, or explicit missing-evidence note.
- Claim gate: packet contains no new root-cause decision, fix plan, verification result, promotion decision, deployment claim, live-proof claim, or completion claim that was not already proven elsewhere.
- Resume gate: packet states the exact next action, safe resume trigger, forbidden claims, and next owner.

Run `node tools/validate-internal-skill-body-quality.mjs --skill slice-debug-handoff-builder` and `node tools/validate-internal-skill-trigger-fixtures.mjs --skill slice-debug-handoff-builder` after changes.

## Failure Modes
Block with `handoff_required` when reproduction is absent without an unable-to-reproduce record, evidence order is missing, hypotheses are not tracked, root cause is unknown but closure is requested, proof is stale, authority is missing, environment or live access is unavailable, the parent Slice cannot be identified, or the next action belongs to another variant.

Escalate instead of continuing debug when three or more fixes have failed, the fix requires redesign, production recovery is the actual work, live user impact dominates, or source truth changed since the debug artifacts were written.

If the packet cannot prove a claim from an artifact, keep the claim in a missing-evidence state and route it to the next owner. If existing artifacts conflict, record the conflict and require a refresh before resume. If the handoff would become a result, promotion, maintenance, source mutation, branch/worktree mutation, deployment, or live-system action, stop and route to the owning step.
