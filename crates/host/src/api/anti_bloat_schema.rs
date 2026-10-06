use super::candidate_schema;
use super::catalog_support::{RouteSpec, uuid};
use crate::tools::object_schema;
use serde_json::json;

macro_rules! route {
    ($tool:expr, $name:expr, $internal:expr, $summary:expr, $conditions:expr,
     $effects:expr, $retry:expr, $schema:expr, $example:expr $(,)?) => {
        RouteSpec {
            tool: $tool,
            route: $name,
            internal: $internal,
            summary: $summary,
            conditions: $conditions,
            effects: $effects,
            retry: $retry,
            schema: $schema,
            example: $example,
        }
    };
}

pub(super) fn routes(example_id: &str) -> Vec<RouteSpec> {
    vec![
        route!(
            "command",
            "scope.anti_bloat.prepare",
            "anti_bloat_prepare",
            "Prepare an auditable source-bound anti-bloat review for one exact candidate-set revision.",
            "Requires the owning open native session, authoritative frozen source binding, and current candidate-set revision.",
            "Saves a deterministic local review; disabled, skip, and no-eligible states never call a provider.",
            "One-shot prepare creates a new review ID on each call; retain the returned ID for recovery.",
            object_schema(
                json!({"candidate_set_id":uuid(),"expected_revision":{"type":"integer","minimum":1},"request_preference":{"type":"string","enum":["use_workspace","skip"]}}),
                json!(["candidate_set_id", "expected_revision"])
            ),
            json!({"candidate_set_id":example_id,"expected_revision":3,"request_preference":"skip"}),
        ),
        route!(
            "query",
            "scope.anti_bloat.get",
            "anti_bloat_get",
            "Read one saved anti-bloat review and its send state.",
            "Requires the original owner in the review workspace and the review ID.",
            "Returns graph digests, findings, state and ranked IDs without raw transport bytes.",
            "Safe to repeat by review ID.",
            object_schema(json!({"review_id":uuid()}), json!(["review_id"])),
            json!({"review_id":example_id}),
        ),
        route!(
            "command",
            "scope.anti_bloat.run",
            "anti_bloat_run",
            "Run the one-use ranking attempt for a prepared anti-bloat review.",
            "Requires the original owner, prepared state and current frozen source/plan binding. Provider transport is disabled by default.",
            "Commits request bytes and send fence before transport; commits raw response before interpretation. Failure remains send_unknown.",
            "Repeat by review ID to read terminal state; sending and send_unknown are never resent.",
            object_schema(json!({"review_id":uuid()}), json!(["review_id"])),
            json!({"review_id":example_id}),
        ),
        route!(
            "command",
            "scope.anti_bloat.apply",
            "anti_bloat_apply",
            "Apply one explicit candidate removal after source-bound preservation checking.",
            "Requires original owner, exact review/finding, narrow disposition, and caller-authored candidate delta with CAS revision and idempotency key.",
            "Checks preservation and saves the full derived after graph as a new authoritative draft at the next candidate-set revision; provider output never authors a mutation.",
            "Repeat only with the same review, finding and exact delta idempotency key; changed input conflicts.",
            object_schema(
                json!({"review_id":uuid(),"finding_id":{"type":"string","pattern":"^[0-9a-f]{64}$"},"disposition":{"type":"string","enum":["narrow"]},"delta":candidate_schema::delta_apply()}),
                json!(["review_id", "finding_id", "disposition", "delta"])
            ),
            json!({"review_id":example_id,"finding_id":"a".repeat(64),"disposition":"narrow","delta":{"candidate_set_id":example_id,"expected_revision":3,"idempotency_key":"narrow-1","operations":[{"operation":"candidate.remove","candidate_id":example_id,"expected_revision":1}]}}),
        ),
        route!(
            "query",
            "scope.anti_bloat.preservation.get",
            "anti_bloat_preservation_get",
            "Read independent-verifier evidence for one native anti-bloat saved-draft mutation.",
            "Requires an authenticated Verifier principal, open session and membership in the exact review workspace; the principal and session must differ from the owner, selected disposition and selected caller.",
            "Re-reads the immutable review, authored delta and caller receipt, source obligations, and both actual saved drafts. Returns the full material with a server-computed evidence digest and current preservation verdict; it does not attest or mutate the plan.",
            "Safe to repeat while the candidate set remains at the applied revision; use the returned evidence digest for verify.",
            object_schema(json!({"review_id":uuid()}), json!(["review_id"])),
            json!({"review_id":example_id}),
        ),
        route!(
            "command",
            "scope.anti_bloat.preservation.verify",
            "anti_bloat_preservation_verify",
            "Append an independent, server-derived preservation attestation.",
            "Requires the same Verifier task/workspace and independence checks as preservation.get, exact review ID, a new request ID and the evidence digest read from that exact saved graph. Stale revisions and foreign reviews are denied.",
            "Recomputes full-graph preservation from persisted source and drafts, then appends one immutable pass, fail or unknown attestation with reason and evidence references. The request cannot supply a verdict and this route does not mutate the plan.",
            "Exact request replay returns the existing attestation; changed review, digest, principal or session conflicts.",
            object_schema(
                json!({"request_id":uuid(),"review_id":uuid(),"expected_evidence_digest":{"type":"string","pattern":"^[0-9a-f]{64}$"}}),
                json!(["request_id", "review_id", "expected_evidence_digest"]),
            ),
            json!({"request_id":example_id,"review_id":example_id,"expected_evidence_digest":"a".repeat(64)}),
        ),
    ]
}
