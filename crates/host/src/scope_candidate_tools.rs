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
        after: Option<i64>,
        limit: u32,
    },
    Fragment {
        candidate_set_id: Uuid,
        source_ref_id: Uuid,
        cursor: usize,
    },
    Begin(BeginCandidateSet),
    SaveDraft(SaveCandidateDraft),
    Review(ReviewCandidateSet),
    RecordInput(RecordCandidateInput),
    Refresh(RefreshCandidateSet),
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
    source_ref_id: Uuid,
    cursor: usize,
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
                ContextArguments::Fragment { args } => Ok(ScopeCandidateInvocation::Fragment {
                    candidate_set_id: args.candidate_set_id,
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
        _ => Err(Error::InvalidArguments),
    }
}

fn page(args: PageArguments, view: CandidateContextView) -> Result<ScopeCandidateInvocation> {
    Ok(ScopeCandidateInvocation::Context {
        candidate_set_id: args.candidate_set_id,
        view,
        after: args.after,
        limit: args.limit,
    })
}

fn decode<T: for<'de> Deserialize<'de>>(arguments: Value) -> Result<T> {
    serde_json::from_value(arguments).map_err(|_| Error::InvalidArguments)
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
        for invalid in [
            json!({"candidate_set_id":id,"view":"overview","limit":25,"after":null}),
            json!({"candidate_set_id":id,"view":"fragment","source_ref_id":id,"cursor":0,"limit":25}),
            json!({"candidate_set_id":id,"view":"reviews"}),
        ] {
            assert_eq!(
                parse("candidate_context", invalid).err(),
                Some(Error::InvalidArguments)
            );
        }
        let review = json!({"kind":"review","candidate_set_id":id,"revision":2,
            "snapshot_id":id,"input_cursor":1,"request_id":id,"review":{"revision":3,
                "verdict":"ready","summary":"reviewed","findings":[],"candidate_decisions":[]}});
        assert_eq!(
            parse("save_candidate_set", review).err(),
            Some(Error::InvalidArguments)
        );
    }
}
