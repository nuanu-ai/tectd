# Research questions and context

Use at R02 after the research contract exists. Read its exact version and the currently delivered DK context, previous research and source checkpoint if present. Do not reconstruct missing sources from chat memory.

Turn the main question into a finite set of answerable questions. For each assign a stable question ID, why it matters, downstream decision or claim it supports, scope, assumptions, sufficient answer/proof, minimum confidence justified by the use, and the condition that would make an answer inadequate. Separate factual inventory, comparisons, source authority, freshness, risk and unresolved user preference. An excluded question remains explicitly excluded; a missing answer is not an exclusion.

Load context at the declared topic level: parent intent, constraints, accepted decisions, current baseline, prior findings, known negative results and open issues. Give every material context item a locator/exact pin where available, source role, authority, freshness and allowed-use limit. Distinguish a canonical unit from a generated index, raw evidence, prior synthesis, historical session pointer and assumption. State when no relevant context exists rather than filling the gap with generic background.

Write `research-questions.md` with the question table, decision links, acceptance/proof/confidence criteria, assumptions, exclusions and likely source/freshness/disconfirmation needs. Write `research-context.md` with loaded and missing context, accepted constraints, freshness/authority limits, contradictions, restricted-source handling and the downstream implication of each gap.

Set `question_coverage` and `context_limits` fields to concise, inspectable descriptions. Every material part of R01 must map to a question or explicit exclusion. If the source question itself changes, return to R01 and invalidate downstream work; do not quietly answer a more convenient question. Otherwise hand these records to source and evidence planning.
