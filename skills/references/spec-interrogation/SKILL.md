---
name: spec-interrogation
description: Use after brainstorming has produced a source design or product specification and before cross-cutting review. Atomizes every normative source requirement, assigns ownership, interrogates implementation-significant decisions, and creates traceable positive and negative acceptance obligations without silently filling product ambiguity.
---

# Specification Interrogation

## Objective

Turn a high-level source specification into explicit component decisions while
proving that no normative source requirement disappeared. The output is not
complete because it is long or internally coherent. It is complete only when
every in-scope source obligation has a stable identity, an accountable owner,
an observable contract, and a decision trail.

This stage runs after brainstorming and before `spec-cross-cutting-review`.

## Non-Negotiable Invariant

Build the source-requirement inventory before interrogating components.
Component decomposition must consume that inventory; it must never replace it.

An unowned, ambiguous, untestable, or unmapped in-scope requirement cannot be
marked Complete. Repeated boilerplate is not evidence of coverage.

## Required Inputs

- The original user request, PRD, design brief, and accepted brainstorming
  decisions.
- Explicit scope changes and human approvals.
- Existing architecture, schemas, and component boundaries when present.
- The directory where decision and traceability artifacts must be written.

If the original source is missing, stale, or cannot be identified, stop and
request the exact source. Do not reconstruct source truth from downstream
component files.

## Required Outputs

Create or update:

- human-readable component decision files;
- `requirements-ledger.json`;
- `decision-traceability.json`;
- `acceptance-obligations.json`.

Use the schemas in `${CODEX_HOME:-$HOME/.codex}/custom-spec-shared/CONTRACT.md`. Preserve the
original source reference and modality in every requirement row.

## Procedure

### 1. Freeze the Source Boundary

Record the source path or paths and a content digest in
`requirements-ledger.json.source`. List every document included in the review
in the human-readable interrogation summary.

Do not silently add a later document to the source set. If the source changes,
refresh the inventory and record that the prior coverage result is stale.

### 2. Atomize Normative Requirements

Read the complete source from start to finish. Extract each independently
testable obligation into one row. Include explicit MUST, SHOULD, and MAY
language and implicit obligations required to make an explicit promise true.
Label an inferred obligation as inferred in its text and cite the exact source
that necessitates it.

Assign a stable requirement ID such as `REQ-ORDERING-001`. IDs remain stable
when wording changes. Do not reuse a removed ID for another meaning.

Populate `sourceRequirementIds` before component work begins. Then create one
`requirements` row for every ID with:

- exact `sourceRef`;
- faithful atomic `text`;
- original `modality`;
- `scope`;
- accountable `owner` or blocking ambiguity;
- concrete `observableOutcomes`;
- concrete `negativeCases`;
- current `status`.

Split conjunctions when either clause can fail independently. Keep one row
when splitting would destroy an atomic invariant.

### 3. Establish Scope and Deferral Authority

Use `scope: "in"` unless an accepted source decision explicitly excludes the
requirement. A difficulty, unknown implementation, time concern, or preference
is not authority to change scope.

Use `status: "deferred"` only after a human-approved scope change. Record
`approvedBy`, `rationale`, and the approving `sourceRef`. Otherwise use
`ambiguous`, `untestable`, or `gap` and block completion.

Never weaken MUST to SHOULD or MAY while clarifying implementation details.

### 4. Map Requirements to Components

Create the component map from behavior and ownership, not file names alone.
For every in-scope requirement, name the primary owner and all collaborating
components. Check boundaries across:

- APIs, events, commands, and schemas;
- state transitions and persistence;
- tenancy, identity, authorization, and data visibility;
- money, quantities, rounding, units, and selection rules;
- idempotency, replay, deduplication, and material-input conflicts;
- failure semantics, retries, compensation, and rollback;
- durable side effects and forbidden side effects;
- asynchronous queues, ordering, concurrency, and time;
- frontend states, user-visible IDs, reasons, and recovery actions;
- observability, audit evidence, and acceptance verification.

If two components appear to own the same invariant, resolve the authority or
mark it ambiguous. Shared ownership without a final authority is not coverage.

### 5. Interrogate Decisions

For each component, work requirement by requirement. Ask what must be decided
for an implementer to satisfy the observable and negative cases without
inventing product behavior.

For each decision, capture:

1. Requirement IDs served.
2. Chosen behavior and rejected alternatives.
3. Inputs, outputs, schemas, and validation.
4. State before, transition, state after, and durable effects.
5. Error identity, retryability, and caller-visible result.
6. Idempotent replay behavior and changed-input conflict behavior.
7. Tenant and authorization boundary.
8. Positive success observation.
9. Negative or forbidden-side-effect observation.
10. Verification method and accountable owner.

Ask the human only when the source permits multiple materially different
product behaviors and no accepted decision resolves them. Present the source
evidence, options, and consequence. Do not use a plausible default to make the
document look complete.

### 6. Create Acceptance Obligations

Every non-deferred in-scope requirement needs positive and negative observables.
Create at least one `positive` and one `negative` obligation in
`acceptance-obligations.json` and link both from
`decision-traceability.json`.

An obligation must state:

- what an external observer can see;
- who owns the behavior;
- how it will be verified;
- which requirement IDs it proves.

Avoid obligations such as "works correctly", "handles errors", or "is
secure". Name the concrete value, state, event, error, absence of side effect,
or UI evidence.

### 7. Build Decision Traceability

Create exactly one trace row per non-deferred in-scope requirement. Link it to
the decision file or files that define the behavior and to its acceptance
obligations.

Traceability must be bidirectional: each decision file names the requirement
IDs it serves, and each trace row names the decision files. An orphan decision
may be useful context, but it does not prove source coverage.

### 8. Run the Interrogation Closure Check

Before handoff, verify:

- every `sourceRequirementIds` entry has one ledger row;
- no ledger row lacks a source inventory ID;
- IDs are unique;
- every in-scope row has source reference, modality, owner, positive
  observable, and negative case;
- every non-deferred in-scope row has decision and acceptance mappings;
- every deferred row has explicit human scope authority;
- every ambiguity or gap is listed as blocking;
- human-readable files and JSON sidecars agree.

Use status `covered` only to mean that the design decision and acceptance
contract cover the source obligation. It does not mean code exists or tests
pass.

## Completion Report

Report:

- source documents and digest;
- total source requirements by modality and status;
- covered, deferred-with-authority, and blocking IDs;
- decision and acceptance artifact paths;
- exact human questions still required;
- whether cross-cutting review may begin.

Do not claim interrogation complete while any in-scope requirement is
unowned, ambiguous, untestable, missing a negative case, or absent from
traceability.

## Handoff

Hand the original source, all decision files, and all three JSON sidecars to
`spec-cross-cutting-review`. The reviewer must independently reconstruct source
coverage rather than trusting this stage's inventory.
