use serde::Deserialize;
use serde_json::Value;
use tect_domain::{
    Error, OpenScope, OpenSlice, RecordSliceCandidateInput, RecordSliceResult,
    RefreshSliceCandidateSet, Result, ReviewSliceCandidateSet, SaveSliceCandidateDraft,
    SliceCandidateContextQuery,
};
use uuid::Uuid;

pub(crate) enum SliceInvocation {
    Window {
        request: Box<SliceInvocation>,
        window: crate::planning_read::Window,
        params: Value,
    },
    ScopeContext {
        scope_id: Uuid,
    },
    Pipelines(PipelineView),
    CandidateContext(SliceCandidateContextQuery),
    OpenScope(OpenScope),
    SaveDraft(SaveSliceCandidateDraft),
    Review(ReviewSliceCandidateSet),
    RecordInput(RecordSliceCandidateInput),
    Refresh(RefreshSliceCandidateSet),
    OpenSlice(OpenSlice),
    SliceContext {
        slice_id: Uuid,
    },
    RecordResult(RecordSliceResult),
}

#[derive(Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PipelineView {
    #[default]
    Full,
    Summary,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PipelineArguments {
    #[serde(default)]
    view: PipelineView,
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

pub(crate) fn parse(name: &str, mut arguments: Value) -> Result<SliceInvocation> {
    if matches!(
        name,
        "scope_context" | "slice_candidate_context" | "slice_pipelines"
    ) && ["offset_bytes", "limit_bytes", "representation_digest"]
        .iter()
        .any(|field| arguments.get(field).is_some())
    {
        let params = arguments.clone();
        let window = crate::planning_read::extract(&mut arguments)?;
        return Ok(SliceInvocation::Window {
            request: Box::new(parse(name, arguments)?),
            window,
            params,
        });
    }
    reject_optional_nulls(&arguments)?;
    match name {
        "scope_context" => {
            decode(arguments).map(
                |args: ScopeContextArguments| SliceInvocation::ScopeContext {
                    scope_id: args.scope_id,
                },
            )
        }
        "slice_pipelines" => {
            decode(arguments).map(|args: PipelineArguments| SliceInvocation::Pipelines(args.view))
        }
        "slice_candidate_context" => {
            let query: SliceCandidateContextQuery = decode(arguments)?;
            if query.after.is_some_and(|after| after < 0) || query.limit == 0 || query.limit > 100 {
                return Err(Error::InvalidArguments);
            }
            Ok(SliceInvocation::CandidateContext(query))
        }
        "scope_open" => decode(arguments).map(SliceInvocation::OpenScope),
        "save_slice_candidate_set" => match decode(arguments)? {
            SaveArguments::Draft { request } => Ok(SliceInvocation::SaveDraft(request)),
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

    #[test]
    fn slice_routes_use_strict_known_shapes() {
        let id = "00000000-0000-4000-8000-000000000001";
        assert!(matches!(
            parse("slice_pipelines", json!({})),
            Ok(SliceInvocation::Pipelines(PipelineView::Full))
        ));
        assert!(matches!(
            parse("slice_pipelines", json!({"view":"summary"})),
            Ok(SliceInvocation::Pipelines(PipelineView::Summary))
        ));
        assert!(parse("scope_context", json!({"scope_id":id})).is_ok());
        assert!(parse("slice_context", json!({"slice_id":id})).is_ok());
        assert!(parse("slice_pipelines", json!({"extra":true})).is_err());
        assert!(parse("slice_pipelines", json!({"view":null})).is_err());
        assert!(parse("slice_pipelines", json!({"view":"unknown"})).is_err());
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
