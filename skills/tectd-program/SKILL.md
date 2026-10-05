---
name: tectd-program
description: Open or continue a TectD Program by turning a broad project narrative into a durable strategy-level PRD through the backend-supplied Program step contract.
---

# TectD Program

Shape one broad project into a coherent Program PRD and persist the work through TectD's Program tools. A Program captures the highest-level product intent and strategy. It stays above Scope and does not create an implementation plan, launch repository work, or imply authorization for delivery.

## Use the backend-owned route

Use only the exact calls, arguments, revision, formats, and next-step instruction supplied by the current TectD system. Program work uses `command` routes `program.begin`, `program.save`, and `program.record_input`; `query` routes `program.get` and `program.list`; and `help` describe for this embedded method. Their live contracts are authoritative. Do not reconstruct route schemas from this skill.

The backend owns the Program ID, workspace association, revision, progress, and `current_step`. It chooses the current step and supplies this one Program method. Follow that compact instruction, loading a referenced method with `help` describe when directed. Never invent, skip, or rewrite the next route.

Start a record when the user is opening a new Program. Resume an existing record when the request or supplied context identifies one; use listing only when necessary to resolve which Program the user means. Reload current state before continuing work that may have been touched by another session or agent.

## Accept a narrative, then shape it

A free-form narrative is normal and sufficient input. Reuse the user's current and previously persisted context. Do not turn opening a Program into a fixed questionnaire, require one human answer per step, or ask the user to repeat information already available.

## Use strategic durable knowledge

Read the backend-supplied Program knowledge manifest and its declared needs before shaping the PRD. It contains reviewed Program-level briefs from applicable published knowledge, with exact revision references, conditions, exceptions and unresolved needs. A saved snapshot tells you what was issued; its digest does not prove that you applied it correctly. A read-only reload reports changes but does not refresh the snapshot.

At this level use business purpose, strategic constraints, accepted high-level decisions and criteria for choosing a direction. Do not copy library versions, commands, infrastructure inventories or detailed implementation plans into Program fields simply because the underlying unit contains them. When a technical detail changes the strategic decision, retain the reviewed strategic consequence and its necessary conditions; refer to the precise underlying resource rather than importing its whole body.

Supply only known task subjects through the advertised typed context. Unknown targets/environments/actions stay unknown; do not declare an empty set or invent IDs to make a required selector disappear. Resolve a meaningful `needs_context` through the supplied input or refresh action. An empty knowledge base permits bootstrap; it is not a reason to manufacture company policies.

Use the exact consumed-manifest guard supplied for saving. If its required knowledge changed, refresh through the backend command, reconsider affected PRD concerns, preserve user inputs and retry with the new guard. Knowledge does not silently override the user's intent or authorize a Program revision: state a material contradiction and obtain the missing decision when necessary. Source references and the current PRD remain distinct records.

Structure the narrative into exactly these PRD concerns:

- `name`: a concise identity for the Program.
- `intent`: the change or outcome the Program exists to pursue and why it matters.
- `basis`: user-provided facts, observed context, cited sources, and clearly labeled working rationale supporting the Program.
- `boundaries`: what is in scope and what is out of scope at Program altitude.
- `constraints`: explicit limits, commitments, dependencies, or non-negotiable conditions.
- `success`: observable outcomes that would demonstrate the Program achieved its intent.

If no explicit constraints are known, state that honestly in `constraints`; do not invent limits to fill a field.

There is no separate description field. Keep the PRD coherent as a whole rather than treating these concerns as isolated form answers. Fill reasonable gaps with working assumptions when doing so preserves the user's intent and does not make a material product choice for them.

Distinguish three kinds of content in the draft: what the user stated, what is supported by verified evidence, and what the agent proposes or assumes. Never convert a proposal into a user decision, invent facts or sources, or manufacture metrics. Preserve the user's explicit constraints and intent even when improving structure or wording.

## Develop and persist the draft

Use `working_notes` as a short continuation summary of decisions, assumptions, meaningful options, and unresolved points. It is not a duplicate PRD and must remain useful to a later session. Original user inputs belong in the backend's separate input history through the supplied `command` route `program.record_input`; do not replace them with summaries or copy them only into `working_notes`.

For both creation and continuation, send the complete original user message as `input`, including its request phrasing and context. Do not extract only the answer fragment, trim the message, or paraphrase it. The structured PRD is where interpretation belongs; the original message remains verbatim.

Read every required original-input page before deciding that a question is necessary or the PRD is complete. Use the backend-supplied consumed-input cursor when saving; never jump to `latest_input` without reading the intervening entries. Normal continuation does not require replaying already incorporated history.

Brainstorm only as much as the Program needs. A coherent draft may emerge in one pass. Complex Programs may develop across many exchanges or sessions. Present the evolving PRD as a whole when feedback would help the user judge its coherence. Do not demand separate approval for every reasonable assumption.

Ask a question only when a material choice remains and no responsible working assumption is available. Keep `pending_question` null otherwise. Before yielding for a necessary answer, first save the best current partial PRD, refreshed `working_notes`, and the exact `pending_question`. This makes the continuation durable even if the next exchange occurs in another session.

While editing an open Program, keep its last coherent required fields available. Put unresolved alternatives in notes and the pending question until they can be incorporated coherently.

When the user responds, preserve the original response separately, reload the latest Program if required by the supplied contract, incorporate the answer, clear or replace the resolved pending question, and save the updated coherent whole. Do not re-ask settled questions.

On a stale revision, follow the backend-provided reload procedure. Compare the fresh record with the attempted update, merge compatible changes, retain all user input, and retry only through the supplied contract. Never overwrite another agent's work or silently discard either version.

## Opening condition

The Program record has only `draft` and `open` states. Continue saving a draft while the PRD is materially incomplete or a critical question remains. When all required PRD concerns form a coherent whole and there is no critical `pending_question`, save it as complete through the current step contract. The backend opens the same Program; present the whole saved PRD, with substantial assumptions or additions visible, and do not add a mandatory final approval ceremony.

Opening confirms that the strategy-level PRD is ready to guide later work. It does not create a Scope, start coding, produce an implementation plan, or authorize execution.

Persist through MCP only. Do not create a client-side `PRD.md`, use the former WorkOrder/preflight/temporary-file/promote/close/release lifecycle, or introduce Epochs, registers, companion-file ceremony, or mandatory review gates.


## Effective save contract and downstream success sources

Before the first `program.save`, read its exact schema with `help` describe and use the supplied save action. `arguments.params.success` contains observable outcomes that demonstrate the Program achieved its intent, for example “A user can complete the stated workflow and its recorded acceptance checks pass.” There is no `program_success` argument. A later frozen planning snapshot classifies this field's issued source reference as `program_success`; finite goals use that exact success source reference and quote. Ongoing planning goals use an issued `planning_input` source reference.

The six PRD fields are `name`, `intent`, `basis`, `boundaries`, `constraints`, and `success`. Each save patch may omit a field to preserve it, set null to clear it, or supply a string to replace it. A partial Draft may save with `complete:false` while fields remain absent. `complete:true`, or editing an already Open Program, requires all six fields nonblank and every original input incorporated through the current consumed-input cursor. `complete:true` also requires no pending question. Supply the values that are still missing in the effective draft before requesting completion; do not invent them.

`program.begin` creates a durable Draft and records the original narrative. It does not claim that a PRD has been composed or opened. Only a successful save confirms that the submitted patches were saved; only the actual backend Open status confirms that the Program is open.
