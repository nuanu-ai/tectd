# Knowledge Change: review and reconciliation

Read the exact intent, baseline, branch plan, evidence, semantic diff, domain receipts and impact plan. Reconstruct the requested meaning from the source rather than trusting the producer's summary. Check lost conditions/exceptions, changed modality, wrong identity/scope, unsupported inference, stale evidence, missing profile, authority expansion and unhandled consumers/copies.

Separate structural validity from semantic sufficiency. A valid RDF payload, filled schema or successful test does not prove the business claim. Read receipts confirm protocol-bound reading, not reasoning quality. Review must explain the substantive checks and cite exact artifacts/versions. An independent reviewer is required only by the applicable policy; changing the name of the same actor does not satisfy such a policy.

For every finding identify the affected operation/obligation, its requirement, evidence, owner phase KC-02–KC-07 and the needed correction. Return it to that phase. Preserve the finding identity and corrective phase across attempts. An unresolved finding may remain in another honest waiting checkpoint; omitting it does not resolve it. After correction, read the new version from that corrective phase and close the finding against its actual output, newer than the unresolved finding. A digest from an unrelated phase or from before the reported defect is insufficient. Ready and no_change require closure of prior unresolved findings; a rejected proposal may retain unresolved findings without presenting them as fixed. A clean review still records what was checked and why the result is ready.

Ready binds one changeset digest, plan revision, source/read set and authority basis. No_change must identify the existing eligible result that already achieves intent; the backend rechecks those guards at completion. Rejected means the proposal was considered and refused, not that publication succeeded. A missing research answer normally leaves the Change waiting on a precise dependency.

If a current guard changes, reconcile the new baseline instead of silently changing expected revisions. After canonical commit, a semantic correction requires a new Change; retry only an outstanding effect. Do not repeat the same review indefinitely without changed input/evidence or a concrete correction hypothesis.

Source: design §7.10 KC-08, §7.13, §7.15. This method does not invoke a development pipeline or external audit harness.
