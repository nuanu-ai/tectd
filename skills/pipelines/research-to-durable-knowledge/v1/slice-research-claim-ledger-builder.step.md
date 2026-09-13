---
id: "slice-research-claim-ledger-builder"
kind: "internal_step"
owner_family: "slice_variant"
pipeline_id: "slice.research-to-durable-knowledge"
step_id: "slice-research-claim-ledger-builder"
entry_gate: false
public_trigger: false
native_skill_discovery: false
body_path: "capabilities/instructions/pipelines/slice-variants/research-to-durable-knowledge/slice-research-claim-ledger-builder.step.md"
source_manifest: "capabilities/pipelines/slice-variants/research-to-durable-knowledge.pipeline.json"
legacy_skill_ref: "slice-research-claim-ledger-builder"
architecture_refs:
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#research-to-durable-kb-slice-variant"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19"
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#procedure-and-research-slice-variants"
---

# Slice Research Claim Ledger Builder

## Overview
This skill turns extracted research claims into a claim-level ledger that future contradiction review, synthesis, and promotion gates can audit.

The core rule is claim granularity: every claim must keep its source, evidence class, freshness, confidence, contradiction posture, unresolved question, durable KB candidate, and forbidden-promotion state separate.

This skill does not decide canonical durable knowledge.

## When to Use
Use this after `slice-research-claim-extractor` has produced candidate claims for the Research To Durable Knowledge Slice Variant and before contradiction/gap classification, synthesis, durable object modeling, or promotion.

Use it when research work selected through `tect-work` needs evidence to become auditable durable-knowledge candidates without losing weak, stale, contradicted, or forbidden material.

Do not use it to:
- gather new evidence;
- rewrite the evidence corpus;
- synthesize findings;
- resolve contradictions;
- promote durable KB;
- update indexes or front doors.

If claims have not been extracted, route back to extraction. If the request is a simple current-state answer, route to query. If the user asks to mutate durable knowledge, route to the promotion gate or owning stateful-domain pipeline.

## Source Contract
Ground this skill in `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#research-to-durable-kb-slice-variant`, `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#procedure-and-research-slice-variants`, `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6`, and `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19`.

The owning manifest is `capabilities/pipelines/slice-variants/research-to-durable-knowledge.pipeline.json`, step `slice-research-claim-ledger-builder`.

The step consumes `extracted-claims.md`, produces `claim-ledger.md`, satisfies gate `claim_ledger_complete_or_gap_recorded`, and reaches terminal state `ready_for_next_step` only when each extracted claim is ledgered, rejected, blocked, deferred, or gap-recorded.

Atom anchors are `pipeline.slice.research-to-durable-knowledge`, `pipeline.slice.research_to_durable_knowledge.claim.ledger.builder`, and `capabilities/pipelines/slice-variants/research-to-durable-knowledge.pipeline.json#step_graph.steps.slice-research-claim-ledger-builder`.

## Operating Procedure
1. Require and load `extracted-claims.md`, then load the research questions, source map, evidence corpus index, evidence log, source-provenance labels, and freshness-authority labels. If the intermediate carrier or any required source is missing, route back to extraction or stop with a gap record instead of inventing claim support.
2. Assign stable claim IDs scoped to the Slice, such as `C-001`. Keep one atomic assertion per row; split bundled claims until support, contradiction, deferral, or rejection can be decided independently.
3. Build the ledger schema before filling rows. Include `claim_id`, `claim`, `source_refs`, `source_role`, `evidence_class`, `freshness`, `confidence`, `contradiction_ref`, `unresolved_question_ref`, `durable_kb_candidate`, `forbidden_promotion`, `allowed_use`, `use_limits`, `status`, and `next_action`.
4. Classify evidence separately from confidence. Evidence class can be direct evidence, derived interpretation, historical snapshot, user-stated input, live/runtime proof, source-only material, restricted evidence, or missing proof.
5. Classify freshness separately from evidence. Freshness can be current, historical, stale-risk, expired, needs-refresh, source-date-unknown, or not-time-sensitive.
6. Set claim status as `supported`, `weak_support`, `contradicted`, `gap_recorded`, `blocked`, `deferred`, `rejected`, `source_only`, or `promotion_candidate`. A claim can become a promotion candidate only after evidence, provenance, freshness, and authority are sufficient.
7. Write `forbidden_promotion` whenever a claim is contradicted, stale-risk without refresh, source-only, restricted, missing authority, bundled, privacy-limited, unsafe to publish, or dependent on unresolved proof.
8. Link uncertainty to stable contradiction, gap, or unresolved-question handles, such as `K-001` or `G-001`. Record the missing source, conflicting source IDs, next proof needed, owner or route, and whether the later classifier must expand it.
9. Add a durable target candidate only as a proposal hint: target object type, likely owner domain, candidate page, runbook, protocol, decision, issue, or follow-up Scope, plus the required promotion gate. Do not write durable KB.
10. Close the ledger with counts by status, blocked and deferred reasons, unresolved questions, forbidden-promotion totals, and the handoff for contradiction classification and synthesis.

## Outputs
The required output is `claim-ledger.md`, derived from `extracted-claims.md`. Add exact line `gate: claim_ledger_complete_or_gap_recorded` only after every extracted row has a ledger disposition or explicit gap.

It contains:
- a compact summary;
- a claim table;
- gap, contradiction, and unresolved-question handles;
- source and evidence link references;
- status totals;
- durable target candidate hints;
- forbidden-promotion reasons;
- next-proof requirements.

Each row must include claim ID, claim text, evidence IDs or source links, evidence class, source authority, freshness state, confidence, allowed use, use limits, contradiction or gap link, unresolved question link, durable target candidate, forbidden-promotion flag, status, and next action.

Valid terminal posture is `ready_for_next_step` when the ledger is complete or all missing support is visible as gaps. Otherwise emit a blocked or handoff note naming the missing source, stale label, authority problem, privacy limit, or contradiction that prevents ledger closure.

## Verification
Check that every extracted claim appears exactly once or is listed as deliberately rejected with reason. Verify that no row lacks evidence links, evidence class, confidence, freshness, status, use limits, and next action. Confirm that unsupported, stale, source-only, restricted, or contradicted claims are not marked as promotion candidates.

Compare the ledger against the source-provenance and freshness-authority files, then scan for bundled claims that need splitting.

Confirm `claim_ledger_complete_or_gap_recorded` by counting extracted claims against ledgered, rejected, blocked, deferred, and gap-recorded rows.

Verify that every promotion candidate has:
- sufficient evidence;
- source provenance;
- freshness or explicit non-time-sensitivity;
- authority;
- no unresolved contradiction;
- no forbidden-promotion flag.

Finally, verify the file contains no canonical durable KB mutation, no promotion approval, no index update, no synthesis claim, and no execution of researched code.

## Failure Modes
Block or hand off when extracted claims are missing, evidence IDs cannot be traced, source-provenance or freshness labels are absent, authority is unresolved, privacy restrictions prevent citation, or a claim depends on live/current truth that has not been refreshed. Record the precise missing proof rather than lowering confidence silently.

Route back to evidence collection or extraction when the claim set is incomplete.

Route to contradiction-and-gap classification when conflicts are visible but ledgerable.

Route to synthesis only after all claims are accounted for and forbidden-promotion rows are visible.

Route to promotion gate or a stateful-domain owner only after the ledger is complete; this skill must not promote, mutate durable KB, rewrite indexes, execute researched code, or hide deferred work.

Keep zero direct write authority for durable knowledge, and preserve the blocked reason so later maintenance can repair, retire, or re-run the research safely.
