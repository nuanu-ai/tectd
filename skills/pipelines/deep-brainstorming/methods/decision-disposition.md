# Decision disposition

Use at B08 after challenge/reconciliation. Read the immutable requested outcome, current recommendation, resolved findings and actual owner authority/answers.

Use decisions and permissions already supplied. If the owner delegated this choice, select within that boundary and explain why. If the request was only for a recommendation, provide the best supported recommendation without pretending the owner adopted it. Ask only for a missing preference, commitment or authority that materially determines the requested decision. Do not require another approval solely because this is a pipeline phase.

Set one disposition: `selected` for a supported choice made under actual authority; `recommended` for a completed recommendation request; `rejected` for an authorized, reasoned decision to decline the considered direction; `pending_decision` when the required owner choice is unresolved. Rejected is not a euphemism for an unanswered question or an exhausted agent.

Record selected/recommended option IDs, the exact scope of the disposition, authority/decision source, rationale, applicable conditions, unresolved limits, expiry/revisit triggers and what commitment has or has not been made. Decision authority does not itself authorize deployment, spending, publication or implementation outside the task.

Write `decision-disposition.md` and set `disposition`, `authority_basis` and `conditions`. For a decision request, a recommendation awaiting adoption remains `pending_decision` with WaitingInput at B08. For a recommendation request, `recommended` may proceed. A pending state must name the exact question, owner and resume condition; it cannot advance as completed merely because a memo has been written.

Return through the owning earlier phase if the owner's answer changes criteria or premises. Continue to synthesis only for a disposition supported by the frozen contract and current reviewed basis. No durable record is published by setting this field.
