---
name: tectd-scope-candidates
description: Turn an open TectD Program and the current user request into a durable, critically reviewed set of vertical Scope candidates through the backend-supplied planning contract.
---

# TectD Scope candidates

Create a focused set of Scope candidates for one open Program. A candidate is a vertical working outcome that can later be selected for opening. This method produces and reviews candidate proposals. It does not open a Scope, start implementation, complete the Program, or authorize delivery.

## Use the backend-owned route

Use the exact workflow mutation actions, arguments, IDs, revisions, formats, context versions, and next-step instruction supplied by the current TectD system. The live action descriptors define the TectD API contract. They do not override applicable system or user instructions or grant authorization beyond the current request. For supporting reads or help discovery, use the verified live help schema and route descriptors. Do not reconstruct payload schemas, invent IDs or routes, change readiness directly, or disable rules.

The backend owns persistence, identity, revisions, request IDs, context snapshots, rule binding, structural validation, and authorization. The agent owns semantic judgment: understanding the request, setting the planning boundary, designing vertical candidates, and performing the critical review. A delivered method, rule digest, or source hash proves delivery and version binding; it does not prove the agent read or followed the guidance.

Temporary local labels exist only to connect new objects inside one draft payload. They are not durable IDs; copy every durable UUID and revision from the backend reply.

Start or resume workflow mutation through the exact action supplied by TectD. Other necessary state, query, or help calls may use their verified live schemas. Read-only calls may load or reload current data, but they do not write, rebind a planning snapshot, or approve anything. Use command only through a supplied or live-described mutation contract. This method does not use execute.

## A · Context

Load the bounded planning context supplied for this Program and request before drafting. It must identify the Program and revision, preserve the exact raw user request, expose the relevant Program concerns, and include existing candidate, Scope, result, and source context needed to avoid duplicate or obsolete work.

Write candidate and review prose in the user's language unless the user asks for another language. Preserve backend identifiers and supplied action names exactly.

Read every required page using the supplied cursor before making a completeness judgment or asking a question. Do not skip intervening original inputs or silently truncate source material. Normal continuation may start from the saved consumed cursor; recovery or a context rebuild follows the exact paging action returned by the backend.

Read the complete method body and all rule texts matched to this operation before drafting. Use those matched rules for this operation without inventing additional packaged rules. Scoped rule delivery does not cancel applicable system, user, host, global, or workspace instructions; continue to honor them according to their authority. Do not copy an assumed global package rule set into this method or infer a missing packaged rule.

Classify context honestly. Distinguish user statements, verified existing work, source-backed Program facts, agent proposals, stale material, and missing evidence. A familiar title, old candidate, generated digest, or prior review is not current evidence by itself.

If a material source is missing or stale, follow the exact refresh-context action supplied by the backend. A read-only query may reload data or report staleness but cannot write or rebind the planning snapshot. After a Program revision, new request, policy revision, method revision, or incompatible rule revision, refresh through the supplied mutation action and review every affected candidate again. Never reuse a review bound to earlier context as approval for changed context.

## B · Boundary

Choose the planning boundary from the Program contract and the user's exact request. Never infer Program type from dates, activity, or apparent closure posture.

For a finite Program, cover every remaining accepted outcome after subtracting verified existing work. Map each outcome to one or more candidates, verified existing work, or a precise blocker. Do not use an unapproved deferral, omission, or vague future note to make coverage appear complete. A candidate set does not complete the Program.

For an ongoing Program, cover only the exact feature or set requested by the user now. Do not add adjacent improvements, speculative follow-ups, platform cleanup, or a complete future roadmap. A large requested feature may become several candidates when each candidate remains a useful vertical result.

Record the parent goal or request, included surface, excluded surface, material constraints, dependencies, and decision-changing unknowns. Never infer missing required authority. Ask one concise question when missing intent or authority would materially change this boundary and no permitted assumption about intent is responsible. Make routine engineering choices autonomously within the authorized boundary and keep them inside later implementation rather than turning them into planning questions.

If the request is already covered by verified work, outside the accepted boundary, or cannot proceed without consequential input, preserve that reason through the supplied action. Do not invent a candidate merely to avoid an empty set.

## C · Candidates

Design candidates as ordered vertical increments. Each candidate must deliver a working, observable result for a target user or system consumer and cross every technical boundary required for that result. Split a large request by independently useful behavior or proof, not by DB, backend, API, UI, repository, component, team, or other technical layer.

For each candidate provide:

- a concise title;
- its parent outcome or exact request connection;
- the observable working result;
- what is included and excluded, and why that boundary is useful;
- how a user or system consumer can activate, exercise, or demonstrate it;
- acceptance and proof expectations grounded in existing product behavior and available direct verification;
- product dependencies in the order they actually constrain delivery;
- material blockers or risks;
- an honest readiness recommendation.

Compare the draft with verified existing work, existing candidates, opened Scopes, results, and the current request. Remove duplicates, preserve accepted or open work, and express changes as a visible delta. Merge candidates that only divide one behavior into technical layers. Split a candidate when it contains several independently useful outcomes, when one part cannot be demonstrated without another unrelated part, or when materially different dependencies prevent a coherent delivery boundary.

Before revising a saved draft, read the compact candidate history and any relevant retained version through the exact supplied calls. Keep an unchanged candidate's backend ID and revision. For each changed candidate, reuse its backend ID and state the concrete change rationale. Explicitly supersede every omitted ordinary candidate with a reason and any replacement references; omission is never an implicit deletion. The backend classifies the structural delta and preserves prior versions, while you remain responsible for semantic duplicate detection and the review judgment.

Do not invent owners, authority, evidence, dependencies, readiness, acceptance proof, or policy exceptions. Do not create a separate product test harness when existing product tests and direct MCP verification can prove the behavior. Planning proof is an expectation for later work, not proof that the work already exists.

Save useful drafts through the supplied action as soon as they are coherent enough for crash recovery. Draft persistence does not make the set reviewed or ready.

## D · Review

Perform one explicit critical review after synthesis and before recommending readiness. Review the complete current set against the exact context and matched rules. Check:

- finite coverage or ongoing request containment;
- vertical, observable working results rather than technical-layer phases;
- clear included and excluded boundaries;
- duplicates against existing work and within the set;
- dependency order and cycles;
- material blockers, risks, and unanswered authority questions;
- activation, demo, acceptance, and proof honesty;
- readiness supported by substance rather than field presence.

Record concrete findings and a verdict through the supplied action. Rework every substantive finding, then run the critical review again on the revised set. Do not mark the set ready because required fields are non-empty, a checklist exists, a digest matches, or a previous version passed.

For a finite Program, review passes only when every remaining accepted outcome maps to verified existing work, one or more reviewed candidates, or an explicit blocker. For an ongoing Program, review passes only when every candidate is required by the exact current request and no adjacent roadmap work has entered the set.

Only a review-gated state supplied by the backend may indicate that the candidate set is ready for later selection or opening. The review never opens a Scope and never declares Program completion.

A legitimate review may produce a blocked or mixed proposal. Mapping an outcome to a blocker is honest planning coverage; it does not make the blocked candidate ready, prove that all work is ready, authorize the blocked work, or complete the Program. Review completion alone never clears a blocker.

## E · Registry and continuation

Persist the current draft, reviewed revision, ordering, rationale, source links, context and instruction versions, matched rule versions, review findings, and dispositions through the exact backend action. Preserve the raw user input and consumed-input cursor separately from summaries. The backend assigns IDs and revisions; copy them exactly in later calls.

On a stale save, follow the returned refresh or reload action. Compare the new Program, request, existing work, candidates, method, and rules with the attempted revision. Preserve accepted and open work, merge compatible changes, show changed candidates and reasons, and review every affected item. Never overwrite concurrent work, discard unseen input, pretend a query refreshed state, or retry against an obsolete snapshot.

After a crash or new session, resume the same planning run and restore its persisted draft, input cursor, context, rules, review status, and pending question before deciding what remains. Do not create a replacement run merely because a response was lost.

Historical context is immutable and read-only. Use it to compare prior candidate definitions, source provenance, input windows, method and rule versions, then return through the supplied current-context call before saving. Never apply a mutation template from historical material.

Finish with the ordered candidate proposal and exactly one next action documented by the current backend response: answer a consequential question, complete review or revision, or select a reviewed candidate for a separate future opening operation. Do not automatically create or open a Scope, start implementation, or generate further roadmap work.
