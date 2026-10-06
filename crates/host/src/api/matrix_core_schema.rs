use super::catalog_support::{RouteSpec, uuid};
use super::matrix_task_schema;
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
            "query",
            "matrix.technical.compare",
            "compare_technical_delivery_mechanisms",
            "Compare the two frozen delivery mechanisms using independently approved technical evidence.",
            "Requires the authenticated current task, declaration ancestry, exact operating verification and independent server approval; unknown evidence is unavailable.",
            "Read only; no ranking, advice, provider call or caller effect.",
            "Safe to repeat while all exact source pins remain current.",
            object_schema(
                json!({"task_id":uuid(),"expected_task_revision":{"type":"integer","minimum":1},"operating_verification_digest":{"type":"string","pattern":"^[0-9a-fA-F]{64}$"},"evidence_reference":object_schema(json!({"artifact_id":uuid(),"artifact_version":{"type":"integer","minimum":1},"content_sha256":{"type":"string","pattern":"^[0-9a-fA-F]{64}$"}}),json!(["artifact_id","artifact_version","content_sha256"]))}),
                json!([
                    "task_id",
                    "expected_task_revision",
                    "operating_verification_digest",
                    "evidence_reference"
                ])
            ),
            json!({"task_id":example_id,"expected_task_revision":1,"operating_verification_digest":"a".repeat(64),"evidence_reference":{"artifact_id":example_id,"artifact_version":1,"content_sha256":"b".repeat(64)}})
        ),
        route!(
            "query",
            "task.source.get",
            "get_matrix_task",
            "Read the current immutable Engineering Matrix source revision for one task.",
            "Requires an authenticated Owner native session bound to the task's workspace.",
            "Reads the current task source revision, recorded identity, digest, and immutable requirements snapshot binding when present. Historical unbound revisions return null binding fields.",
            "Safe to repeat; a missing task returns not_found.",
            object_schema(json!({"task_id":uuid()}), json!(["task_id"])),
            json!({"task_id":example_id}),
        ),
        route!(
            "command",
            "task.source.record",
            "record_matrix_task",
            "Record one exact Engineering Matrix factual input revision and optional Owner-authored or Owner-adopted exact engineering alternatives for a task.",
            "Requires an authenticated open native session, non-nil task and request IDs, revision 1 or the immediate successor of the expected current revision, valid tagged factual input and an optional choice set bound to the exact task/revision. Combined input and choice-set JSON is capped at 1 MiB; at most 1024 reported facts and five candidates. Zero or one candidate is recorded but not eligible for ranking. Choice-set assumptions must reference Matrix fact IDs in this input.",
            "With requirements_locator, freezes current accepted Program/Scope/logical Work declarations in the same transaction, injects absent confirmed fields, and atomically stores their immutable snapshot ID and semantic digest with the source revision. Operating claims remain owner-reported. Without a locator, records a legacy unbound revision.",
            "Repeat the same request_id with identical original revision, input, choice set, and requirements_locator. Bound replay returns its saved snapshot even when current declarations changed. On uncertainty, read task.source.get before another write.",
            object_schema(
                json!({"task_id":uuid(),"revision":{"type":"integer","minimum":1},"expected_current_revision":{"type":"integer","minimum":0},"request_id":uuid(),"input":matrix_task_schema::input(),"choice_set":matrix_task_schema::choice_set(),"requirements_locator":super::matrix_requirements_schema::locator()}),
                json!([
                    "task_id",
                    "revision",
                    "expected_current_revision",
                    "request_id",
                    "input"
                ])
            ),
            json!({"task_id":example_id,"revision":1,"expected_current_revision":0,"request_id":"00000000-0000-4000-8000-000000000002","input":matrix_task_schema::example(),"choice_set":matrix_task_schema::example_choice_set(example_id)}),
        ),
        route!(
            "command",
            "engineering.matrix.verify",
            "verify_matrix_task",
            "Verify the saved Engineering Matrix facts against immutable evidence references.",
            "Requires an authenticated verifier native session bound to the workspace, an exact saved task revision and input digest, and one evidence reference per required fact. The verifier must differ from the owner who recorded the revision. Evidence validation is disabled by default.",
            "On successful evidence validation, appends a sealed verification record. The response is a bounded digest and per-fact status receipt without raw evidence content.",
            "On uncertainty, inspect the saved task revision; avoid retrying with changed evidence. Verification remains unavailable until a trusted evidence validator is configured.",
            object_schema(
                json!({
                    "task_id":uuid(),
                    "expected_revision":{"type":"integer","minimum":1},
                    "input_digest":{"type":"string","pattern":"^[0-9a-fA-F]{64}$"},
                    "evidence":{"type":"array","maxItems":1040,"items":object_schema(
                        json!({"fact_path":{"type":"string","minLength":1,"maxLength":512,"pattern":"^/"},"evidence_ref":{"type":"string","minLength":1,"maxLength":4096,"pattern":"\\S"}}),
                        json!(["fact_path","evidence_ref"])
                    )}
                }),
                json!(["task_id", "expected_revision", "input_digest", "evidence"])
            ),
            json!({"task_id":example_id,"expected_revision":1,"input_digest":"0".repeat(64),"evidence":[{"fact_path":"/mode","evidence_ref":"urn:evidence:example"}]}),
        ),
        route!(
            "query",
            "engineering.matrix.cards.get",
            "get_verified_matrix_cards",
            "Read mandatory Engineering Matrix cards from an exact current V2 independently verified task revision.",
            "Requires authenticated workspace membership, current confirmed declaration ancestry, a fresh approved-artifact verification and its exact digest. Legacy owner-reported cards are never a fallback.",
            "Read only; returns mandatory IDs and summaries plus one requested full body, with frozen V2 provenance but no raw artifact.",
            "Safe to repeat while the exact task, declaration ancestry and operating verification remain current.",
            object_schema(
                json!({"task_id":uuid(),"expected_task_revision":{"type":"integer","minimum":1},"operating_verification_digest":{"type":"string","pattern":"^[0-9a-fA-F]{64}$"},"card_id":{"type":"string","minLength":1,"maxLength":128}}),
                json!([
                    "task_id",
                    "expected_task_revision",
                    "operating_verification_digest"
                ])
            ),
            json!({"task_id":example_id,"expected_task_revision":1,"operating_verification_digest":"a".repeat(64),"card_id":"EM02-SCOPE@0.1"})
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn core_routes() -> Vec<RouteSpec> {
        let id = "00000000-0000-4000-8000-000000000001";
        let mut result = routes(id);
        result.extend(super::super::matrix_requirements_schema::routes(id));
        result
    }

    #[test]
    fn matrix_core_catalogue_exposes_exactly_eight_provider_free_routes() {
        let mut actual: Vec<_> = core_routes()
            .iter()
            .map(|r| (r.tool, r.route, r.internal))
            .collect();
        actual.sort();
        let mut expected = vec![
            (
                "query",
                "matrix.technical.compare",
                "compare_technical_delivery_mechanisms",
            ),
            (
                "command",
                "engineering.matrix.context.propose",
                "matrix_context_propose",
            ),
            (
                "command",
                "engineering.matrix.context.confirm",
                "matrix_context_confirm",
            ),
            (
                "query",
                "engineering.matrix.context.effective.get",
                "matrix_context_effective_get",
            ),
            ("command", "task.source.record", "record_matrix_task"),
            ("query", "task.source.get", "get_matrix_task"),
            ("command", "engineering.matrix.verify", "verify_matrix_task"),
            (
                "query",
                "engineering.matrix.cards.get",
                "get_verified_matrix_cards",
            ),
        ];
        expected.sort();
        assert_eq!(actual, expected);
        assert_eq!(super::super::PUBLIC_TOOLS.len(), 5);
        assert_eq!(super::super::WIRE_API_VERSION, 2);
    }

    #[test]
    fn matrix_core_schemas_and_decoder_reject_caller_authority() {
        for route in core_routes() {
            assert_eq!(route.schema["additionalProperties"], false);
            let mut forged = route.example.clone();
            forged["owner_authorship_ref"] = json!("caller-asserted-authority");
            assert!(
                super::super::decode_public_call(
                    route.tool,
                    json!({"route":route.route,"params":forged})
                )
                .is_err(),
                "{}",
                route.route
            );
        }
        let verify = routes("00000000-0000-4000-8000-000000000001")
            .into_iter()
            .find(|r| r.internal == "verify_matrix_task")
            .unwrap();
        assert_eq!(
            verify.schema["properties"]["evidence"]["items"]["additionalProperties"],
            false
        );
    }

    #[test]
    fn matrix_core_examples_delegate_to_the_exact_internal_parser() {
        for route in core_routes() {
            assert!(
                super::super::decode_public_call(
                    route.tool,
                    json!({"route":route.route,"params":route.example})
                )
                .is_ok(),
                "{}",
                route.route
            );
            assert!(crate::tools::parse_invocation(route.internal, route.example.clone()).is_ok());
        }
    }
}
