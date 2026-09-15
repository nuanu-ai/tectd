# Research result and handoff

Use at R12 after result verification. Compile the highest supported result from current outputs; do not add new evidence, change the question or resolve an outstanding contradiction inside the closeout.

Write `result.md` with the direct answer and intended use, question/scope/exclusions, selected result state, exact synthesis/ledger/verification references, evidence horizon, confidence and applicability, contradictions/negative knowledge, rejected/source-only/restricted/deferred items, unresolved limits and one concrete next step. Make the result usable without rereading the chat to discover a blocker.

`answered` means the contracted questions have a supported answer. `negative_result` means a valid disconfirmation or bounded negative answer, with its inspected scope and limits. `inconclusive` is permitted only by the immutable inquiry and the R09 bounded-inconclusive record; it does not satisfy another task's demand to establish feasibility. `waiting_source` and `waiting_authority` remain WaitingInput with the required input, owner and resume condition. An infrastructure failure to retrieve material is not a negative answer.

Set `result_state`, `next_step` and `publication_status=not_performed`. Terminal Complete requires the corresponding supported R09/R11 disposition and all exact phase bindings; waiting states never receive a completed Slice result.

For checkpoint-driven Research, identify the exact originating checkpoint, question and answer criteria and explain how the result informs them, including unresolved criteria. The result becomes eligible for explicit acceptance by the waiting Brainstorming run through `slice.pipeline.checkpoint.resolve`; it does not itself select an option or mark that Brainstorming complete. Preserve exact terminal output/result IDs instead of a mutable document link.

Optionally identify worthwhile durable candidates by exact operational output/claim references, scope, source provenance, restrictions and maintenance needs. A separate Promotion/Knowledge Change performs object modeling, domain obligations, review, authority and publication/effects. Do not create a fake publisher receipt or claim discoverability/vector readiness from a research result. If no candidate merits retention, record that outcome without blocking this answer.
