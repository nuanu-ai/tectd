---
name: spec-cross-cutting-review
description: Use after specification interrogation and before reconciliation. Independently reconstructs normative requirements from the original source, performs a read-only adversarial coverage and consistency review across components, and reports omissions, contradictions, ambiguity, untestability, or unauthorized deferral without repairing the reviewed artifacts.
---

# Cross-Cutting Specification Review

## Objective

Detect requirements lost between the original source and the interrogated
component package, even when every generated file is internally consistent.
Consistency among downstream files is not evidence that those files are
complete.

This stage is read-only with respect to the original source, decision files,
and JSON sidecars. It writes the review report only. `spec-reconciliation`
owns repairs.

## Required Inputs

- The exact original source set and digest used by interrogation.
- Human-approved scope changes.
- All component decision files.
- `requirements-ledger.json`.
- `decision-traceability.json`.
- `acceptance-obligations.json`.
- The shared contract at
  `${CODEX_HOME:-$HOME/.codex}/custom-spec-shared/CONTRACT.md`.

If source identity or digest differs from the interrogation record, stop with a
stale-source finding. Do not compare artifacts from different source versions.

## Output

Write `cross-cutting-review.md` containing:

- source identity and digest;
- an independently reconstructed source inventory;
- a requirement-by-requirement coverage matrix;
- cross-component findings grouped by dimension;
- blocking IDs and evidence;
- a clean or findings-present verdict;
- the exact handoff to reconciliation.

Do not edit decision files or JSON sidecars during review.

## Finding Statuses

Classify every independently identified source requirement as exactly one of:

- `COVERED`: faithful decision, owner, and acceptance evidence exists.
- `CONTRADICTION`: downstream artifacts disagree with the source or each other.
- `GAP`: all or part of the source obligation is missing.
- `AMBIGUOUS`: multiple material behaviors remain possible without authority.
- `UNTESTABLE`: no concrete observable or verification method proves it.
- `DEFERRED`: an explicit human-approved scope change is present and cited.

Do not use `COVERED` for plausible intent, repeated prose, file presence, or a
generic acceptance statement.

## Procedure

### 1. Verify Review Boundary

Record source paths, digest, reviewed artifact paths, and current revision.
Confirm the review report is a new output and the remaining artifacts are
read-only. If another process changes them during review, invalidate the
verdict and restart from a stable snapshot.

### 2. Source Coverage Gate

Before reading `requirements-ledger.json`, independently reconstruct the
normative requirement inventory from the original source. Read the source in
full and record temporary review IDs, exact references, original modality, and
atomic obligation text.

Then open the ledger and semantically diff the two inventories. Check:

- source requirement missing from `sourceRequirementIds`;
- inventory ID with no requirement row;
- two source obligations incorrectly merged into one row;
- one obligation split in a way that loses an invariant;
- source modality weakened or scope changed;
- implicit but necessary negative behavior omitted;
- source requirement mapped to the wrong component or no final authority.

The independently reconstructed inventory controls the review. Do not declare
the source complete merely because ledger IDs are internally balanced.

### 3. Traceability Gate

For every independently reconstructed in-scope requirement, verify:

1. A faithful ledger row exists.
2. The row retains exact source reference and modality.
3. A primary owner is accountable for the final invariant.
4. Decision files define concrete behavior.
5. Decision traceability points to those files.
6. Positive and negative acceptance obligations exist.
7. Obligations name observable evidence and a verification method.
8. Any deferral cites a human-approved scope change.

Follow links in both directions. Detect decision files that claim requirement
IDs absent from the ledger and obligations that point to unknown requirements.

### 4. Adversarial Cross-Cutting Dimensions

Review the entire package across these dimensions. For each finding, cite
source requirement IDs, components, artifact references, and an observable
failure scenario.

#### Authority and Ownership

- Which component is final authority for each invariant?
- Can two components accept conflicting state?
- Are validation and authorization performed by the authoritative boundary?

#### Contracts and Schemas

- Are request, response, event, command, and error variants complete?
- Are required fields, enums, units, nullability, and version behavior exact?
- Do producers and consumers agree on semantics, not just field names?

#### State and Durability

- Are all allowed and forbidden transitions specified?
- Which writes, events, and external effects are atomic or eventually durable?
- Does rejection guarantee absence of forbidden side effects?
- Are compensation, reversal, and rollback states represented?

#### Tenancy, Identity, and Authorization

- Are reads and writes both tenant scoped?
- Can identifiers from another tenant influence selection or mutation?
- Are denial errors and no-side-effect guarantees explicit?

#### Money, Quantity, and Selection

- Are currency, units, precision, rounding, thresholds, and selection bands
  defined?
- Are credits, reversals, and negative adjustments represented where required?
- Are boundary values and tie-breaking behavior observable?

#### Idempotency and Conflict

- Does identical replay return the prior result?
- Does reuse of an idempotency key with materially changed input produce a
  distinct conflict?
- Is the material-input fingerprint defined across service boundaries?

#### Failure and Recovery

- Are stable error identities, retryability, and caller behavior defined?
- Are partial failures, timeouts, poison messages, and terminal failures
  distinguishable?
- Can recovery duplicate or erase durable effects?

#### Async and Concurrency

- Are ordering, deduplication, race behavior, retry limits, and stale updates
  explicit?
- Can concurrent commands violate ownership or state-transition invariants?

#### Frontend and Operator Evidence

- Can the user see state, reason values, queue or audit identifiers, and next
  actions required by the source?
- Are loading, empty, denied, conflict, failed, and recovered states defined?
- Does UI evidence correspond to backend truth rather than local optimism?

#### Verification Quality

- Does every obligation prove a concrete source behavior?
- Are positive and negative checks both present?
- Can a test pass while the source requirement is still violated?
- Are forbidden durable side effects inspected explicitly?

### 5. Build the Coverage Matrix

Include one row per independently reconstructed source requirement:

| Review ID | Ledger ID | Modality | Source ref | Owner | Decision refs | Positive proof | Negative proof | Status | Finding |
|---|---|---|---|---|---|---|---|---|---|

Use `GAP` when there is no ledger ID. Do not omit the row.

### 6. Determine Verdict

A clean verdict requires:

- 100% of non-deferred in-scope MUST requirements are `COVERED`;
- every in-scope SHOULD and MAY is `COVERED` or has explicit human-approved
  disposition;
- zero `CONTRADICTION`, `GAP`, `AMBIGUOUS`, or `UNTESTABLE` rows;
- every `DEFERRED` row has complete authority evidence;
- no orphan or unknown traceability links;
- positive and negative obligations agree with the source behavior.

File count, line count, test count, and repeated template sections do not
contribute to this verdict.

## Finding Format

For each non-covered row record:

- finding ID and severity;
- status;
- source requirement and exact reference;
- affected components and artifacts;
- current downstream behavior;
- required source behavior;
- concrete failure or false-pass scenario;
- artifacts reconciliation must update;
- whether a human product decision is required.

Do not prescribe implementation details unless the source already requires
them. Review identifies the contract failure; reconciliation decides the
coherent repair.

## Handoff

Always hand off to `spec-reconciliation`, even after a clean verdict. A clean
review requires reconciliation to run closure-audit mode and produce evidence;
findings require findings-resolution mode.
