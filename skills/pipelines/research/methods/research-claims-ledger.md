# Research claims and evidence ledger

Use at R07 after custody and source qualification. First extract atomic claims; then account for all of them in the ledger. These are separate obligations even though they share a phase.

Read the question set and qualified evidence, including negative/gap notes. Split material assertions until support, uncertainty, rejection or applicability can be decided independently. Preserve normative modality, quantifiers, conditions, exceptions, dates and environment: 'may', 'must', 'some' and 'all' are not interchangeable. Use stable claim IDs and distinguish observed fact, source assertion, derived inference, requirement, decision, risk, constraint, comparison, negative finding and open question.

Produce `extracted-claims.md`. Each row contains the atomic statement, type, question, cited evidence IDs and locators, source role, provenance, authority, freshness, confidence, applicability/scope, allowed use, contradiction/gap signals and required next proof. An inference identifies its premises and reasoning; an unsupported idea remains a hypothesis or question. Do not extract new factual claims from uncited memory or generated summaries without their parents.

Produce `claim-ledger.md` with every extracted row given a disposition: supported, weak support, contradicted, gap-recorded, blocked, deferred, rejected or source-only. Preserve claim text and IDs through adjudication. Link evidence class, confidence, freshness and scope separately; attach stable conflict/gap/question handles, use limits, next action and owner. Reconcile `extracted_count = accounted_count` using disjoint dispositions so rejected and blocked claims cannot disappear.

Record publication eligibility only as a conditional hint. Mark forbidden-publication reasons for unresolved contradiction, stale unsupported currentness, source-only material, missing authority, unsafe/private content, bundled claims or unresolved proof. No durable candidate is approved here. Optional target hints can name a decision record, runbook, protocol, finding or other unit only if they help later reuse; a target lane is not required for an answer.

Set `extracted_count`, `accounted_count`, `claim_trace_complete=true` only when each claim has evidence or an explicit missing-proof disposition and all extracted rows are accounted for. Return to R06 for missing labels/custody. Keep competing evidence visible for R08; do not choose a convenient winner or silently upgrade narrow/historical evidence to broad current truth.
