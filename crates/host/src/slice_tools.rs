use serde::Deserialize;
use serde_json::Value;
use tect_domain::{
    Error, OpenScope, OpenSlice, RecordSliceCandidateInput, RecordSliceResult,
    RefreshSliceCandidateSet, Result, ReviewSliceCandidateSet, SaveSliceCandidateDraft,
    SliceCandidateContextQuery,
};
use uuid::Uuid;

pub(crate) enum SliceInvocation {
    ScopeContext { scope_id: Uuid },
    Pipelines,
    CandidateContext(SliceCandidateContextQuery),
    OpenScope(OpenScope),
    SaveDraft(SaveSliceCandidateDraft),
    Review(ReviewSliceCandidateSet),
    RecordInput(RecordSliceCandidateInput),
    Refresh(RefreshSliceCandidateSet),
    OpenSlice(OpenSlice),
    SliceContext { slice_id: Uuid },
    RecordResult(RecordSliceResult),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ScopeContextArguments {
    scope_id: Uuid,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SliceContextArguments {
    slice_id: Uuid,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum SaveArguments {
    Draft {
        #[serde(flatten)]
        request: SaveSliceCandidateDraft,
    },
    Review {
        #[serde(flatten)]
        request: ReviewSliceCandidateSet,
    },
}

pub(crate) fn parse(name: &str, arguments: Value) -> Result<SliceInvocation> {
    reject_optional_nulls(&arguments)?;
    match name {
        "scope_context" => {
            decode(arguments).map(
                |args: ScopeContextArguments| SliceInvocation::ScopeContext {
                    scope_id: args.scope_id,
                },
            )
        }
        "slice_pipelines" if empty_object(&arguments) => Ok(SliceInvocation::Pipelines),
        "slice_candidate_context" => {
            let query: SliceCandidateContextQuery = decode(arguments)?;
            if query.after.is_some_and(|after| after < 0) || query.limit == 0 || query.limit > 100 {
                return Err(Error::InvalidArguments);
            }
            Ok(SliceInvocation::CandidateContext(query))
        }
        "scope_open" => decode(arguments).map(SliceInvocation::OpenScope),
        "save_slice_candidate_set" => match decode(arguments)? {
            SaveArguments::Draft { request } => {
                if let Some(selection) = &request.matrix_selection {
                    selection.validate()?;
                }
                Ok(SliceInvocation::SaveDraft(request))
            }
            SaveArguments::Review { request } => Ok(SliceInvocation::Review(request)),
        },
        "record_slice_candidate_input" => decode(arguments).map(SliceInvocation::RecordInput),
        "refresh_slice_candidate_set" => decode(arguments).map(SliceInvocation::Refresh),
        "slice_open" => decode(arguments).map(SliceInvocation::OpenSlice),
        "slice_context" => {
            decode(arguments).map(
                |args: SliceContextArguments| SliceInvocation::SliceContext {
                    slice_id: args.slice_id,
                },
            )
        }
        "slice_result_record" => decode(arguments).map(SliceInvocation::RecordResult),
        _ => Err(Error::InvalidArguments),
    }
}

fn decode<T: for<'de> Deserialize<'de>>(arguments: Value) -> Result<T> {
    serde_json::from_value(arguments).map_err(Error::invalid_arguments_from)
}

fn empty_object(value: &Value) -> bool {
    value.as_object().is_some_and(serde_json::Map::is_empty)
}

fn reject_optional_nulls(value: &Value) -> Result<()> {
    const NON_NULL: &[&str] = &[
        "after",
        "local",
        "candidate_id",
        "revision",
        "change_rationale",
        "why_lightweight_insufficient",
        "why_further_vertical_split_not_viable",
        "consumed_knowledge",
        "task_context",
        "matrix_selection",
        "disposition_id",
    ];
    match value {
        Value::Object(object) => {
            if object
                .iter()
                .any(|(key, value)| NON_NULL.contains(&key.as_str()) && value.is_null())
            {
                return Err(Error::InvalidArguments);
            }
            for value in object.values() {
                reject_optional_nulls(value)?;
            }
        }
        Value::Array(values) => {
            for value in values {
                reject_optional_nulls(value)?;
            }
        }
        _ => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn draft_arguments() -> Value {
        let id = "00000000-0000-4000-8000-000000000001";
        json!({"kind":"draft","scope_id":id,"candidate_set_id":id,
            "revision":1,"snapshot_id":id,"input_cursor":1,"request_id":id,
            "draft":{"coverage_summary":"Complete coverage","nodes":[{"kind":"work",
                "identity":{"local":"first"},"title":"Bounded outcome","outcome":"Delivered",
                "proof":["Direct evidence"],"pipeline":"slice.lightweight-tdd-development",
                "pipeline_reason":"Bounded development"}]}})
    }

    fn matrix_selection() -> Value {
        let id = "00000000-0000-4000-8000-000000000001";
        let digest = "a".repeat(64);
        json!({"task_id":id,"task_revision":1,"disposition_id":id,
            "selected_choice_id":"choice-a","mapped_draft_node_indices":[0],
            "expected_input_digest":digest,
            "expected_choice_set_digest":digest,"expected_verification_digest":digest})
    }

    #[test]
    fn save_draft_accepts_plain_and_explicit_matrix_provenance() {
        let plain = draft_arguments();
        let SliceInvocation::SaveDraft(request) =
            parse("save_slice_candidate_set", plain.clone()).unwrap()
        else {
            panic!("expected draft")
        };
        assert!(request.matrix_selection.is_none());

        let mut with_selection = plain;
        with_selection["matrix_selection"] = matrix_selection();
        let SliceInvocation::SaveDraft(request) =
            parse("save_slice_candidate_set", with_selection).unwrap()
        else {
            panic!("expected draft")
        };
        assert_eq!(
            request.matrix_selection.unwrap().selected_choice_id,
            "choice-a"
        );
    }

    #[test]
    fn save_draft_rejects_invalid_matrix_provenance() {
        let mut request = draft_arguments();
        request["matrix_selection"] = matrix_selection();
        for (key, invalid) in [
            ("task_id", json!("not-a-uuid")),
            ("task_revision", json!(0)),
            ("mapped_draft_node_indices", json!([-1])),
            ("mapped_draft_node_indices", json!([0.5])),
            ("mapped_draft_node_indices", json!([])),
            ("mapped_draft_node_indices", json!([0, 0])),
            ("mapped_draft_node_indices", json!([2, 0])),
            ("expected_input_digest", json!("not-a-digest")),
            ("expected_choice_set_digest", json!("A".repeat(64))),
            ("expected_verification_digest", json!("0".repeat(63))),
            ("unknown", json!(true)),
        ] {
            let mut invalid_request = request.clone();
            invalid_request["matrix_selection"][key] = invalid;
            assert!(
                parse("save_slice_candidate_set", invalid_request).is_err(),
                "accepted invalid matrix field {key}"
            );
        }
        request["matrix_selection"] = Value::Null;
        assert!(parse("save_slice_candidate_set", request).is_err());

        let mut missing_native_target = draft_arguments();
        missing_native_target["matrix_selection"] = matrix_selection();
        missing_native_target
            .as_object_mut()
            .unwrap()
            .remove("scope_id");
        assert!(parse("save_slice_candidate_set", missing_native_target).is_err());
    }

    #[test]
    fn slice_routes_use_strict_known_shapes() {
        let id = "00000000-0000-4000-8000-000000000001";
        assert!(parse("slice_pipelines", json!({})).is_ok());
        assert!(parse("scope_context", json!({"scope_id":id})).is_ok());
        assert!(parse("slice_context", json!({"slice_id":id})).is_ok());
        assert!(parse("slice_pipelines", json!({"extra":true})).is_err());
        assert!(parse("scope_context", json!({"scope_id":id,"extra":true})).is_err());
        assert!(parse("slice_context", json!({"slice_id":id,"extra":true})).is_err());
        assert!(
            parse(
                "slice_candidate_context",
                json!({
                    "scope_id":id,"view":"overview","limit":25,"after":null
                })
            )
            .is_err()
        );
    }

    #[test]
    fn full_pipeline_requires_both_exceptional_choice_explanations() {
        let id = "00000000-0000-4000-8000-000000000001";
        let value = json!({"kind":"draft","scope_id":id,"candidate_set_id":id,
            "revision":1,"snapshot_id":id,"input_cursor":1,"request_id":id,
            "draft":{"coverage_summary":"Full coverage","nodes":[{"kind":"work",
                "identity":{"local":"full"},"title":"Complex outcome","outcome":"Delivered",
                "proof":["Direct evidence"],"pipeline":"slice.full-design-to-execution",
                "pipeline_reason":"Complex"}]}});
        let SliceInvocation::SaveDraft(request) = parse("save_slice_candidate_set", value).unwrap()
        else {
            panic!("expected draft")
        };
        assert_eq!(request.draft.validate(), Err(Error::InvalidArguments));
    }
}
