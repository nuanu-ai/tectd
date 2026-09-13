---
name: spec-synthesis
description: Use after reconciliation has produced a ready closure record. Compiles source requirements and reconciled decisions losslessly into an implementation-ready specification, preserves normative modality and observable contracts, emits requirement-to-section traceability, and runs final validation before planning.
---

# Specification Synthesis

## Objective

Produce one implementation-ready specification that an engineer can execute
without rereading the entire decision history, while preserving every accepted
source obligation. Synthesis is lossless compilation, not free paraphrase.

Compression may remove repetition. It may not remove requirements, modalities,
error variants, boundary behavior, ownership, acceptance evidence, or forbidden
side effects.

## Entry Gate

Required inputs:

- the exact original source set and digest;
- all reconciled component decision files;
- `cross-cutting-review.md` with finding dispositions;
- `requirements-ledger.json`;
- `decision-traceability.json`;
- `acceptance-obligations.json`;
- `reconciliation-closure.json` with `completionClaim: "ready"` and no
  unresolved IDs;
- a green reconciliation-stage validator result.

If reconciliation is blocked, stale, or missing evidence, do not synthesize.
Return the package to reconciliation with exact requirement IDs.

## Required Outputs

Create or replace:

- `implementation-ready-spec.md`;
- `synthesis-traceability.json`.

Do not modify source requirements or decision authority during synthesis. If a
necessary change is discovered, return it to reconciliation.

## Compilation Invariants

1. Every non-deferred in-scope requirement appears in the synthesized spec.
2. Every requirement has at least one exact section reference in
   `synthesis-traceability.json`.
3. Preserve the original normative modality: MUST remains MUST, SHOULD remains
   SHOULD, and MAY remains MAY.
4. Every owner, input, output, state transition, error, durable effect, and
   acceptance obligation remains semantically intact.
5. A negative guarantee stays explicit. "Reject" must not replace "reject and
   create no order, billing event, notification, or other forbidden effect"
   when the latter is the accepted contract.
6. A generic statement cannot replace enumerated values, variants, selection
   rules, boundary conditions, or UI evidence.
7. Deferred requirements remain visibly deferred with their human authority;
   they are not presented as implemented behavior.

## Procedure

### 1. Verify Source and Closure

Compare the current source digest to the ledger and review. Confirm
reconciliation mode, validator result, and empty unresolved list. Verify that
no input artifact changed after the reconciliation proof.

If any proof is stale, stop. Do not regenerate a reassuring narrative from an
unverified mixture of revisions.

### 2. Build the Synthesis Checklist

Create an internal checklist from every non-deferred in-scope ledger row. For
each requirement record:

- ID, modality, and source reference;
- primary owner and collaborators;
- decision references;
- positive and negative obligation IDs;
- target section or sections in the implementation-ready spec.

The checklist is requirement-centric. Component headings are a presentation
structure, not the coverage authority.

### 3. Write the Implementation-Ready Structure

Use the smallest structure that preserves all contracts. Include these sections
when applicable:

1. Purpose, scope, non-goals, and source provenance.
2. System invariants and requirement index.
3. Component ownership and trust boundaries.
4. API, command, event, and data contracts.
5. State machines and durable persistence behavior.
6. Tenancy, identity, authorization, and visibility rules.
7. Money, quantity, units, rounding, thresholds, and selection behavior.
8. Idempotency, replay, deduplication, and changed-input conflict semantics.
9. Failure, retry, compensation, reversal, and rollback behavior.
10. Async ordering, concurrency, and recovery.
11. Frontend states and operator-visible evidence.
12. Observability, audit, and acceptance matrix.
13. Explicit deferrals and residual risks.

Use stable anchors so `synthesis-traceability.json.sectionRefs` points to exact
locations.

### 4. Compile Exact Behavioral Contracts

For every requirement, preserve implementation-significant precision:

- request fields, response fields, variants, enums, units, and nullability;
- authoritative validation and ownership boundary;
- state before, transition, state after, and illegal transitions;
- stable error identity and retryability;
- identical replay result and material-input conflict result;
- tenant filtering for reads as well as writes;
- atomic, eventual, compensating, and forbidden side effects;
- boundary values, selection rules, credits, and reversals;
- queue, reason, audit, and correlation IDs visible to users or operators;
- positive success observation and negative no-side-effect observation.

Do not replace exact behavior with "appropriate", "as needed", "handles",
"supports", or "works correctly".

### 5. Resolve Duplication Without Losing Meaning

When several decisions repeat one invariant, state it once in the authoritative
section and link consumers to it. Before deleting repeated text, compare every
variant for unique fields, exceptions, modalities, and negative guarantees.

If variants disagree, stop and return the contradiction to reconciliation.
Synthesis cannot choose which accepted decision wins.

### 6. Emit Synthesis Traceability

Create one row per non-deferred in-scope requirement in
`synthesis-traceability.json` with:

- `requirementId`;
- exact preserved `modality`;
- one or more `sectionRefs`.

Check both directions:

- every required ledger ID has a synthesis row;
- every synthesis row points to a known non-deferred requirement;
- every section reference exists and contains the promised behavior;
- each section names the requirement IDs it implements or proves.

Set `completionClaim: "blocked"` until final validation passes.

### 7. Run Semantic Pressure Checks

Before the machine validator, manually pressure the synthesized spec with at
least these questions:

- Can another tenant's identifier affect a read, selection, or write?
- What happens on identical replay, and what happens when material input
  changes under the same command ID?
- Are monetary or quantity boundary and reversal cases explicit?
- Can a rejected flow still create any durable downstream effect?
- Can asynchronous retry duplicate a committed effect?
- Can the UI expose the state, reason, and identifiers needed to act?
- Can every MUST be converted directly into positive and negative acceptance
  checks?

Any uncertain answer is a requirement-level blocker, not an invitation to add
plausible prose.

### 8. Run Final Validation

Run:

```bash
node "${CODEX_HOME:-$HOME/.codex}/custom-spec-shared/validate-spec-pipeline.js" .
```

Resolve all errors through the owning stage. Fix mapping or section-reference
errors in synthesis. Return source, scope, decision, ownership, modality, or
acceptance defects to reconciliation.

After validation returns `valid: true`, set
`synthesis-traceability.json.completionClaim` to `ready` and rerun the validator
to prove the final bytes.

## Completion Gate

Synthesis is ready for planning only when:

- source and reconciliation evidence are fresh;
- every non-deferred in-scope requirement is mapped;
- modality is preserved exactly;
- all referenced sections exist and contain concrete behavior;
- positive and negative obligations remain implementable;
- no contradiction or ambiguity was smoothed over;
- final validation returns `valid: true` against the final artifacts.

Document length, polished prose, section count, and downstream test count do not
prove this gate.

## Completion Report

Report:

- source digest and reconciliation proof consumed;
- requirement totals by modality and disposition;
- synthesized requirement and section counts;
- explicit deferrals with authority;
- validator command and final result;
- exact artifact paths;
- whether implementation planning may begin.

## Handoff

Hand `implementation-ready-spec.md`, all five JSON sidecars, and the final
validator result to the planning workflow. Planning must retain requirement IDs
in tasks and acceptance checks so implementation evidence can be mapped back to
the source contract.
