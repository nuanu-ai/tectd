---
name: spec-reconciliation
description: Use after every cross-cutting specification review, including a clean review. Resolves findings or performs a closure audit, updates human and machine traceability artifacts consistently, requires explicit authority for scope changes, and blocks synthesis until reconciliation-stage validation proves closure.
---

# Specification Reconciliation

## Objective

Convert the independent cross-cutting review into a coherent, source-faithful
specification package. This skill must always run after
`spec-cross-cutting-review`, even when the review reports no findings.

Reconciliation owns repair. Review is read-only; synthesis cannot reinterpret
unresolved findings.

## Modes

Choose exactly one mode and record it in `reconciliation-closure.json`:

- `findings-resolution`: one or more review rows are `CONTRADICTION`, `GAP`,
  `AMBIGUOUS`, `UNTESTABLE`, or invalid `DEFERRED`.
- `closure-audit`: the review claims clean coverage and reconciliation must
  independently prove that a no-op handoff is justified.

Never skip this skill because the review is clean. Never select closure-audit
to avoid resolving a finding.

## Required Inputs

- The exact original source set and digest.
- Human-approved scope decisions.
- `cross-cutting-review.md`.
- All component decision files.
- `requirements-ledger.json`.
- `decision-traceability.json`.
- `acceptance-obligations.json`.
- The shared contract and validator under
  `${CODEX_HOME:-$HOME/.codex}/custom-spec-shared`.

If source digest differs from the reviewed digest, invalidate the review and
return to interrogation. Do not reconcile across source versions.

## Required Outputs

Create or update:

- affected component decision files;
- `requirements-ledger.json`;
- `decision-traceability.json`;
- `acceptance-obligations.json`;
- `reconciliation-closure.json`;
- a reconciliation section or amendment in `cross-cutting-review.md` linking
  every finding to its disposition.

Do not create the final implementation-ready specification in this stage.

## Authority Rules

Reconciliation may clarify implementation behavior only within accepted source
intent. It may not change product scope, weaken modality, or invent a material
business rule.

An in-scope MUST can be deferred only through a human-approved scope change
with `approvedBy`, `rationale`, and authoritative `sourceRef`. Cost, effort,
uncertainty, missing code, or agent preference are not deferral authority.

When a finding exposes a genuine product choice, ask the human with:

1. the exact source evidence;
2. the conflicting downstream statements;
3. materially distinct options;
4. observable consequences and affected requirements.

Keep the requirement blocked until the answer is recorded.

## Procedure

### 1. Verify Inputs and Select Mode

Confirm source identity, digest, review verdict, artifact revision, and absence
of concurrent mutation. Select findings-resolution if any non-covered review
row exists; otherwise select closure-audit.

Create `reconciliation-closure.json` immediately with the selected mode,
`completionClaim: "blocked"`, and all currently unresolved requirement IDs.

### 2. Reproduce Each Finding

For each review finding, read the cited source and downstream artifacts. Do not
accept a finding solely because it appears in the report. Confirm:

- the source obligation and original modality;
- the exact omission, contradiction, ambiguity, or untestable claim;
- affected owners, decisions, obligations, and consumers;
- a concrete scenario that currently violates or falsely passes the contract.

If the finding is incorrect, record evidence and mark it rejected. A rejected
finding still needs a disposition; it must not disappear from the audit trail.

### 3. Build the Resolution Matrix

Maintain one row per finding:

| Finding | Requirement IDs | Root cause | Resolution | Files changed | Acceptance changes | Authority | Status |
|---|---|---|---|---|---|---|---|

Allowed statuses are `resolved`, `rejected_with_evidence`,
`blocked_human_decision`, and `deferred_with_authority`.

Map a source omission back into `sourceRequirementIds` and the ledger before
editing component prose. This prevents a prose-only repair from remaining
invisible to closure checks.

### 4. Repair One Requirement Cluster at a Time

Group only findings that share one invariant. For each cluster, update all
affected artifacts as one logical change:

1. Requirement inventory and faithful ledger row.
2. Primary and collaborating component decisions.
3. Input, output, state, error, tenancy, idempotency, and side-effect rules.
4. Positive and negative acceptance obligations.
5. Decision traceability links.
6. Cross-cutting review disposition.

Do not leave human-readable and JSON artifacts in conflicting states between
clusters. After each cluster, reread the source requirement and its complete
trace path.

### 5. Apply Adversarial Repair Checks

For every resolved cluster, explicitly test the dimensions relevant to it:

- tenant isolation on reads, writes, and identifier lookup;
- identical replay versus changed material-input conflict;
- monetary or quantity boundaries, selection, credit, and reversal behavior;
- transition legality, rollback, compensation, and durable effects;
- rejection with no forbidden side effect;
- stable errors, retryability, and terminal failure;
- async ordering, duplication, races, and stale state;
- UI-visible state, reason, queue or audit ID, and next action;
- positive success and negative acceptance evidence.

A repair is incomplete when it fixes one producer but leaves a consumer,
schema, UI state, or acceptance obligation stale.

### 6. Perform Closure-Audit Mode

When the review is clean, sample-free confidence is required: traverse every
independently reconstructed review row, not a subset. Confirm its source row,
decision, owner, positive obligation, negative obligation, and deferral
authority where applicable.

The validator is mandatory before a no-op closure can be accepted. A report
that merely says "no changes needed" is not evidence.

If closure-audit discovers a gap, switch the recorded mode to
findings-resolution, add a finding, and repair it normally.

### 7. Run Reconciliation Validation

Run:

```bash
node "${CODEX_HOME:-$HOME/.codex}/custom-spec-shared/validate-spec-pipeline.js" . --stage reconciliation
```

Resolve every reported error. Do not edit the validator output or exclude a
requirement to obtain green status. If a validator rule conflicts with source
truth, keep completion blocked and report the contract defect separately.

### 8. Close or Block

Set `completionClaim: "ready"` and clear `unresolvedRequirementIds` only when:

- every review finding has an evidence-bearing disposition;
- every independently reconstructed in-scope requirement is covered or has
  explicit human deferral authority;
- human and machine artifacts agree;
- reconciliation-stage validation returns `valid: true`;
- no human decision remains outstanding.

Otherwise keep `completionClaim: "blocked"` and list exact unresolved IDs.

## Completion Report

Report:

- selected mode;
- source digest and reviewed revision;
- resolved, rejected, deferred-with-authority, and blocked finding IDs;
- requirement IDs and files changed;
- validator command and result;
- unresolved human decisions;
- whether synthesis may begin.

Do not claim the package ready because all files exist, tests elsewhere pass,
or no contradiction is visible. Readiness requires source coverage and a green
reconciliation-stage contract.

## Handoff

Hand the reconciled source package, closure record, and validator result to
`spec-synthesis`. If closure is blocked, return to the responsible decision
owner instead; synthesis must not smooth over the gap.
