---
name: tectd-setup
description: Turn a free-form company or work narrative into one durable AGENTS.md for the actual Codex task launch directory through the backend-supplied setup contract.
---

# Workspace setup

Use the current backend-owned step, exact tool calls, arguments, IDs, revisions and input formats. The live tools are authoritative. This method supplies judgement for composing the file; it does not replace the current route. Keep one setup record from the first narrative through application.

## Use the task's directory

The target is the folder from which the current Codex task was launched. Obtain the absolute path from the task's existing environment context and supply it in `command` route `setup.inspect`. Never ask the human to choose a folder. Do not substitute the MCP package directory, a repository path, a selected worktree, the process working directory, or a new folder. The agent supplies this known context; native-session authentication does not cryptographically attest the path. The backend separately checks the enrolled host's current setup write grant and physical directory identity.

An omitted context, denied access, unsupported path, or failed inspection is unknown or unavailable, never evidence that AGENTS.md is missing. Initial setup is appropriate only after a current verified absence. An existing file belongs to the user and must be preserved. Repository registration and a source read grant do not imply setup write authority. A logical workspace can have zero, one or many registered and selected sources; none is required for this file.

## Compose from what the user already said

Read and preserve all applicable global and local agent instructions while composing. Keep global files and every existing local file unchanged. Preserve the mandatory Tect default for repository work and the owner's explicitly scoped opt-out and lifecycle-authority exceptions; never weaken them through a new local file. Instruction preservation is an agent responsibility checked in the actual scenario, not a database semantic guarantee.

Preserving inheritance means following those instructions, not copying whole global files into the new local file. Keep AGENTS.md focused on this company/work context and local requirements. A concise, accurate reference to the applicable inherited rules is sufficient unless the owner explicitly asks for a full copy.

Accept one complete free-form narrative about the company, project or working context. It is sufficient to start. Persist the entire original user message exactly, including request phrasing and surrounding context, through the supplied begin or record input action. Do not extract an answer fragment, trim, paraphrase or replace original input with notes. Do not run a fixed questionnaire or ask the user to repeat available information.

Write a useful, coherent AGENTS.md for future agents working here. Include relevant company/work context, terminology, responsibilities, goals and practical working instructions only as supported by the narrative and available evidence. Preserve the user's intent and explicit constraints. Clearly distinguish user facts, verified facts and your proposed assumptions; do not invent company facts, files, services, sources, metrics, approvals or product decisions. Do not fill gaps with generic bureaucracy, speculative architecture or an imposed process framework.

Before calling `execute` route `setup.apply`, send a user-visible commentary message containing the entire saved `content` verbatim in a fenced Markdown code block. A tool result alone does not satisfy this presentation; neither does showing the file only after application. Do not trim, paraphrase or replace it with an excerpt. If that exact saved content has already been shown, do not duplicate it. Then apply the same revision under the user's existing setup authorization, without a routine final approval ceremony. The final response can link the created file and report verification without repeating its full body.

Improve clarity and structure while keeping the user's meaning. One pass may be enough; more complex material may need several exchanges. Use reasonable working assumptions for non-material gaps. Ask a concise question only when a material unresolved choice has no responsible default.

Before yielding for a necessary answer, save the best complete or partial `content`, concise continuation `working_notes`, and the exact `pending_question`. Keep notes useful to a new session; do not duplicate the whole file or original input history there. On reply, record the complete original message, incorporate the answer, and clear or replace the question before advancing. Do not re-ask a settled question.

If a requested output-style exception is actually necessary, discuss the concrete reason, the available options, and their token cost with the owner before using it. This is a conditional exception rule, not a general permission gate or a mandatory question for ordinary setup.

## Persist, resume and apply

Read all required original-input pages using the exact returned cursor. In a new session or recovery, follow the supplied `query` route `setup.get` action from `after_input: 0` and continue to the end; restore the whole draft, notes and pending question before deciding what remains. A normal continuation can use the backend's saved consumed-input cursor. Reading old history never permits moving the saved cursor backwards. Never claim unseen input has been incorporated.

The backend owns `compose`, `waiting_input`, `ready_to_apply` and `complete`. Save partial work with `ready: false`. Save `ready: true` only when the whole file is coherent, every original input is incorporated and no pending question remains. On a stale revision reload from the supplied action, reconcile compatible changes and preserve both sessions' original inputs before saving again.

Apply only the durable ready revision through `execute` route `setup.apply`. Do not write the file through a shell, editor, another tool or a client-side state file. The backend creates only the fixed AGENTS.md, never overwrites an existing different file, and verifies the published bytes. A matching file after a lost response can complete the same operation safely. Report creation or successful verification only from a successful current application result; a saved `applied` status or an observation with `observed_now: false` is historical information.

If a result is uncertain, use the exact recovery call. Preserve the same request ID and exact original text for a begin or input retry, or the same setup ID and ready revision for an apply retry. Never create a replacement draft merely because a response was lost. A file changed or removed after application must not be restored from historical state. Preserve a conflicting file and report the current conflict.

The company instructions file and Program PRDs serve distinct purposes. Setup does not create a Program or launch repository work. Existing Programs remain available throughout setup; starting a new Program is the last offered action. Follow the supplied Program calls when the user chooses that work. Do not introduce local state directories, companion artifacts or the former WorkOrder/preflight/promote lifecycle.
