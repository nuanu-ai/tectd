use serde::Deserialize;
use serde_json::Value;
use tect_domain::{
    BeginCandidateSet, CandidateContextView, Error, RecordCandidateInput, RefreshCandidateSet,
    Result, ReviewCandidateSet, SaveCandidateDraft,
};
use uuid::Uuid;

pub(crate) enum ScopeCandidateInvocation {
    Context {
        candidate_set_id: Uuid,
        view: CandidateContextView,
        draft_revision: Option<i64>,
        after: Option<i64>,
        limit: u32,
    },
    Fragment {
        candidate_set_id: Uuid,
        draft_revision: Option<i64>,
        source_ref_id: Uuid,
        cursor: usize,
    },
    Begin(BeginCandidateSet),
    SaveDraft(SaveCandidateDraft),
    Review(ReviewCandidateSet),
    RecordInput(RecordCandidateInput),
    Refresh(RefreshCandidateSet),
    DeltaApply(tect_domain::CandidateDeltaBatch),
    DeltaStatus {
        candidate_set_id: Uuid,
        idempotency_key: String,
    },
}

#[derive(Deserialize)]
#[serde(tag = "view", rename_all = "snake_case", deny_unknown_fields)]
enum ContextArguments {
    Overview {
        #[serde(flatten)]
        args: PageArguments,
    },
    Program {
        #[serde(flatten)]
        args: PageArguments,
    },
    Inputs {
        #[serde(flatten)]
        args: PageArguments,
    },
    Candidates {
        #[serde(flatten)]
        args: PageArguments,
    },
    Reviews {
        #[serde(flatten)]
        args: PageArguments,
    },
    History {
        #[serde(flatten)]
        args: PageArguments,
    },
    Historical {
        #[serde(flatten)]
        args: HistoricalArguments,
    },
    Fragment {
        #[serde(flatten)]
        args: FragmentArguments,
    },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PageArguments {
    candidate_set_id: Uuid,
    #[serde(default)]
    after: Option<i64>,
    limit: u32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FragmentArguments {
    candidate_set_id: Uuid,
    #[serde(default)]
    draft_revision: Option<i64>,
    source_ref_id: Uuid,
    cursor: usize,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct HistoricalArguments {
    candidate_set_id: Uuid,
    draft_revision: i64,
    #[serde(default)]
    after: Option<i64>,
    limit: u32,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum SaveArguments {
    Draft {
        #[serde(flatten)]
        request: SaveCandidateDraft,
    },
    Review {
        #[serde(flatten)]
        request: ReviewCandidateSet,
    },
}

pub(crate) fn parse(name: &str, arguments: Value) -> Result<ScopeCandidateInvocation> {
    reject_optional_nulls(&arguments)?;
    match name {
        "candidate_context" => {
            let args: ContextArguments = decode(arguments)?;
            match args {
                ContextArguments::Overview { args } => page(args, CandidateContextView::Overview),
                ContextArguments::Program { args } => page(args, CandidateContextView::Program),
                ContextArguments::Inputs { args } => page(args, CandidateContextView::Inputs),
                ContextArguments::Candidates { args } => {
                    page(args, CandidateContextView::Candidates)
                }
                ContextArguments::Reviews { args } => page(args, CandidateContextView::Reviews),
                ContextArguments::History { args } => page(args, CandidateContextView::History),
                ContextArguments::Historical { args } => Ok(ScopeCandidateInvocation::Context {
                    candidate_set_id: args.candidate_set_id,
                    view: CandidateContextView::Historical,
                    draft_revision: Some(args.draft_revision),
                    after: args.after,
                    limit: args.limit,
                }),
                ContextArguments::Fragment { args } => Ok(ScopeCandidateInvocation::Fragment {
                    candidate_set_id: args.candidate_set_id,
                    draft_revision: args.draft_revision,
                    source_ref_id: args.source_ref_id,
                    cursor: args.cursor,
                }),
            }
        }
        "begin_candidate_set" => decode(arguments).map(ScopeCandidateInvocation::Begin),
        "save_candidate_set" => match decode(arguments)? {
            SaveArguments::Draft { request } => Ok(ScopeCandidateInvocation::SaveDraft(request)),
            SaveArguments::Review { request } => Ok(ScopeCandidateInvocation::Review(request)),
        },
        "record_candidate_input" => decode(arguments).map(ScopeCandidateInvocation::RecordInput),
        "refresh_candidate_set" => decode(arguments).map(ScopeCandidateInvocation::Refresh),
        "scope_candidate_delta" => decode(arguments).map(ScopeCandidateInvocation::DeltaApply),
        "scope_candidate_delta_status" => {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Status {
                candidate_set_id: Uuid,
                idempotency_key: String,
            }
            let value: Status = decode(arguments)?;
            Ok(ScopeCandidateInvocation::DeltaStatus {
                candidate_set_id: value.candidate_set_id,
                idempotency_key: value.idempotency_key,
            })
        }
        _ => Err(Error::InvalidArguments),
    }
}

fn page(args: PageArguments, view: CandidateContextView) -> Result<ScopeCandidateInvocation> {
    Ok(ScopeCandidateInvocation::Context {
        candidate_set_id: args.candidate_set_id,
        view,
        draft_revision: None,
        after: args.after,
        limit: args.limit,
    })
}

fn decode<T: for<'de> Deserialize<'de>>(arguments: Value) -> Result<T> {
    serde_json::from_value(arguments.clone()).map_err(|reason| {
        let message = reason.to_string();
        let pointer = message
            .strip_prefix("missing field `")
            .and_then(|value| value.split('`').next())
            .and_then(|field| candidate_missing_pointer(&arguments, field));
        if let Some(pointer) = pointer {
            Error::invalid_arguments_at(message, pointer)
        } else {
            Error::invalid_arguments_from(message)
        }
    })
}

fn candidate_missing_pointer(value: &Value, field: &str) -> Option<String> {
    let draft = value.get("draft")?;
    let collections: &[(&str, &[&str])] = &[
        (
            "goals",
            &["identity", "text", "source_ref_id", "resolution"],
        ),
        (
            "evidence",
            &["identity", "kind", "summary", "source_ref_id"],
        ),
        (
            "candidates",
            &[
                "identity",
                "title",
                "outcome",
                "trigger",
                "delivered_behavior",
                "proof",
                "coverage_goals",
            ],
        ),
        ("blockers", &["identity", "summary", "source_ref_id"]),
    ];
    for (collection, required) in collections {
        if required.contains(&field)
            && let Some((index, _)) =
                draft
                    .get(collection)
                    .and_then(Value::as_array)
                    .and_then(|items| {
                        items
                            .iter()
                            .enumerate()
                            .find(|(_, item)| item.get(field).is_none())
                    })
        {
            return Some(format!("/params/draft/{collection}/{index}/{field}"));
        }
    }
    if ["boundary", "goals", "candidates"].contains(&field) && draft.get(field).is_none() {
        return Some(format!("/params/draft/{field}"));
    }
    None
}

fn reject_optional_nulls(value: &Value) -> Result<()> {
    const NON_NULL: &[&str] = &[
        "after",
        "exact_quote",
        "authority_input_sequence",
        "pending_question",
        "empty_disposition",
        "prior_candidate_id",
        "replacement_evidence",
        "target_candidate",
        "local",
        "id",
        "revision",
        "change_rationale",
        "draft_revision",
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
    fn candidate_routes_reject_unknown_missing_and_explicit_null_fields() {
        let id = "00000000-0000-4000-8000-000000000001";
        assert!(
            parse(
                "candidate_context",
                json!({"candidate_set_id":id,"view":"overview","limit":25})
            )
            .is_ok()
        );
        assert!(
            parse(
                "candidate_context",
                json!({"candidate_set_id":id,"view":"historical","draft_revision":2,"limit":25})
            )
            .is_ok()
        );
        assert!(
            parse(
                "candidate_context",
                json!({"candidate_set_id":id,"view":"fragment","draft_revision":2,"source_ref_id":id,"cursor":0})
            )
            .is_ok()
        );
        for invalid in [
            json!({"candidate_set_id":id,"view":"overview","limit":25,"after":null}),
            json!({"candidate_set_id":id,"view":"fragment","source_ref_id":id,"cursor":0,"limit":25}),
            json!({"candidate_set_id":id,"view":"fragment","snapshot_id":id,"source_ref_id":id,"cursor":0}),
            json!({"candidate_set_id":id,"view":"historical","limit":25}),
            json!({"candidate_set_id":id,"view":"reviews"}),
        ] {
            assert_eq!(
                parse("candidate_context", invalid)
                    .err()
                    .map(|error| error.code()),
                Some("invalid_arguments")
            );
        }
        let review = json!({"kind":"review","candidate_set_id":id,"revision":2,
            "snapshot_id":id,"input_cursor":1,"request_id":id,"review":{"revision":3,
                "verdict":"ready","summary":"reviewed","findings":[],"candidate_decisions":[]}});
        assert_eq!(
            parse("save_candidate_set", review)
                .err()
                .map(|error| error.code()),
            Some("invalid_arguments")
        );
    }

    #[test]
    fn nested_candidate_decode_failure_reports_exact_json_pointer() {
        let id = "00000000-0000-4000-8000-000000000001";
        let error = parse(
            "save_candidate_set",
            json!({
                "kind":"draft","candidate_set_id":id,"revision":1,
                "snapshot_id":id,"input_cursor":1,"request_id":id,
                "draft":{
                    "boundary":"finite","goals":[],
                    "candidates":[{
                        "identity":{"local":"candidate_one"},
                        "title":"One","outcome":"Outcome","trigger":"Trigger",
                        "proof":"Proof","coverage_goals":[]
                    }]
                }
            }),
        )
        .err()
        .unwrap();
        let diagnostic = error.argument_diagnostic().unwrap();
        assert_eq!(diagnostic.violation_code, "required_field_missing");
        assert_eq!(
            diagnostic.pointer,
            "/params/draft/candidates/0/delivered_behavior"
        );
    }

    #[test]
    fn save_draft_accepts_a_planning_manifest_guard() {
        let id = Uuid::new_v4();
        let value = json!({
            "kind":"draft","candidate_set_id":id,"revision":1,"snapshot_id":Uuid::new_v4(),
            "input_cursor":1,"request_id":Uuid::new_v4(),
            "consumed_knowledge":{"manifest_id":Uuid::new_v4(),"digest":"digest","workspace_generation":1},
            "draft":{"boundary":"finite","goals":[{"identity":{"local":"goal"},"text":"goal",
              "source_ref_id":Uuid::new_v4(),"resolution":{"kind":"candidate","reference":{"local":"scope"}}}],
              "evidence":[],"candidates":[{"identity":{"local":"scope"},"title":"scope","outcome":"outcome",
              "trigger":"trigger","delivered_behavior":"behavior","proof":"proof","coverage_goals":[{"local":"goal"}]}],
              "blockers":[],"protected_changes":[]}
        });
        assert!(matches!(
            parse("save_candidate_set", value),
            Ok(ScopeCandidateInvocation::SaveDraft(request))
                if request.candidate_set_id == id && request.consumed_knowledge.is_some()
                    && request.draft.validate().is_ok()
        ));
    }

    #[test]
    fn delta_routes_decode_apply_and_status() {
        let id = Uuid::new_v4();
        let apply = json!({"candidate_set_id":id,"expected_revision":2,"idempotency_key":"k",
            "operations":[{"operation":"coverage.link","candidate_id":id,"goal_id":Uuid::new_v4()}]});
        assert!(
            matches!(parse("scope_candidate_delta", apply), Ok(ScopeCandidateInvocation::DeltaApply(request)) if request.expected_revision == 2)
        );
        let status = json!({"candidate_set_id":id,"idempotency_key":"k"});
        assert!(
            matches!(parse("scope_candidate_delta_status", status), Ok(ScopeCandidateInvocation::DeltaStatus { candidate_set_id, .. }) if candidate_set_id == id)
        );
    }
}
