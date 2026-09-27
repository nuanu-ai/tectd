use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tect_application::{
    ConfirmMatrixRequirementsContext, MatrixRequirementsLocator, ProposeMatrixRequirementsContext,
};
use tect_domain::{Error, RequirementDeclarationPatch, Result};
use uuid::Uuid;

const DECLARATION_TEXT_MAX_BYTES: usize = 256;

#[derive(Deserialize, Serialize)]
#[serde(tag = "level", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Locator {
    Program {
        program_id: Uuid,
    },
    Scope {
        program_id: Uuid,
        scope_id: Uuid,
    },
    Slice {
        program_id: Uuid,
        scope_id: Uuid,
        candidate_set_id: Uuid,
        work_candidate_id: Uuid,
        expected_work_revision: i64,
    },
    OpenedSlice {
        slice_id: Uuid,
    },
}
pub(crate) fn parse_locator(value: Value) -> Result<MatrixRequirementsLocator> {
    let locator: Locator = serde_json::from_value(value).map_err(Error::invalid_arguments_from)?;
    let valid = match locator {
        Locator::Program { program_id } => !program_id.is_nil(),
        Locator::Scope {
            program_id,
            scope_id,
        } => !program_id.is_nil() && !scope_id.is_nil(),
        Locator::Slice {
            program_id,
            scope_id,
            candidate_set_id,
            work_candidate_id,
            expected_work_revision,
        } => {
            !program_id.is_nil()
                && !scope_id.is_nil()
                && !candidate_set_id.is_nil()
                && !work_candidate_id.is_nil()
                && expected_work_revision >= 1
        }
        Locator::OpenedSlice { slice_id } => !slice_id.is_nil(),
    };
    if !valid {
        return Err(Error::InvalidArguments);
    }
    Ok(locator.into())
}
impl From<Locator> for MatrixRequirementsLocator {
    fn from(value: Locator) -> Self {
        match value {
            Locator::Program { program_id } => Self::Program { program_id },
            Locator::Scope {
                program_id,
                scope_id,
            } => Self::Scope {
                program_id,
                scope_id,
            },
            Locator::Slice {
                program_id,
                scope_id,
                candidate_set_id,
                work_candidate_id,
                expected_work_revision,
            } => Self::Slice {
                program_id,
                scope_id,
                candidate_set_id,
                work_candidate_id,
                expected_work_revision,
            },
            Locator::OpenedSlice { slice_id } => Self::OpenedSlice { slice_id },
        }
    }
}
impl From<&MatrixRequirementsLocator> for Locator {
    fn from(value: &MatrixRequirementsLocator) -> Self {
        match *value {
            MatrixRequirementsLocator::Program { program_id } => Self::Program { program_id },
            MatrixRequirementsLocator::Scope {
                program_id,
                scope_id,
            } => Self::Scope {
                program_id,
                scope_id,
            },
            MatrixRequirementsLocator::Slice {
                program_id,
                scope_id,
                candidate_set_id,
                work_candidate_id,
                expected_work_revision,
            } => Self::Slice {
                program_id,
                scope_id,
                candidate_set_id,
                work_candidate_id,
                expected_work_revision,
            },
            MatrixRequirementsLocator::OpenedSlice { slice_id } => Self::OpenedSlice { slice_id },
        }
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProposeArguments {
    request_id: Uuid,
    locator: Locator,
    expected_context_revision: u64,
    patches: Vec<RequirementDeclarationPatch>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ConfirmArguments {
    request_id: Uuid,
    locator: Locator,
    proposal_revision: u64,
    proposal_digest: String,
    owner_response_ref: String,
}
#[derive(Deserialize)]
#[serde(
    tag = "kind",
    content = "description",
    rename_all = "snake_case",
    deny_unknown_fields
)]
enum StrictIntent {
    ProductionHotfix,
    Other(String),
}

pub(crate) enum MatrixRequirementsContextInvocation {
    Propose(ProposeMatrixRequirementsContext),
    Confirm(ConfirmMatrixRequirementsContext),
    Get(MatrixRequirementsLocator),
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GetArguments {
    locator: Locator,
}
fn has_overlong_declaration_text(arguments: &Value) -> bool {
    arguments
        .get("patches")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .any(|patch| {
            let value = patch.get("value");
            let kind = value
                .and_then(|value| value.get("kind"))
                .and_then(Value::as_str);
            let text = match kind {
                Some("intent") => value
                    .and_then(|value| value.get("value"))
                    .and_then(|value| value.get("description")),
                Some("urgency" | "promised_behavior" | "promised_proof") => {
                    value.and_then(|value| value.get("value"))
                }
                _ => None,
            };
            text.and_then(Value::as_str)
                .is_some_and(|text| text.len() > DECLARATION_TEXT_MAX_BYTES)
        })
}
pub(crate) fn parse(name: &str, arguments: Value) -> Result<MatrixRequirementsContextInvocation> {
    if name == "matrix_context_propose" {
        if has_overlong_declaration_text(&arguments) {
            return Err(Error::InvalidArguments);
        }
        for patch in arguments
            .get("patches")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if let Some(value) = patch.get("value")
                && value.get("kind").and_then(Value::as_str) == Some("intent")
            {
                let intent: StrictIntent = serde_json::from_value(
                    value.get("value").cloned().ok_or(Error::InvalidArguments)?,
                )
                .map_err(Error::invalid_arguments_from)?;
                if let StrictIntent::Other(description) = intent
                    && description.trim().is_empty()
                {
                    return Err(Error::InvalidArguments);
                }
            }
        }
    }
    if name == "matrix_context_confirm"
        && arguments
            .get("owner_response_ref")
            .and_then(Value::as_str)
            .is_some_and(|text| text.len() > DECLARATION_TEXT_MAX_BYTES)
    {
        return Err(Error::InvalidArguments);
    }
    match name {
        "matrix_context_propose" => {
            serde_json::from_value::<ProposeArguments>(arguments).map(|a| {
                MatrixRequirementsContextInvocation::Propose(ProposeMatrixRequirementsContext {
                    request_id: a.request_id,
                    locator: a.locator.into(),
                    expected_context_revision: a.expected_context_revision,
                    patches: a.patches,
                })
            })
        }
        "matrix_context_confirm" => {
            serde_json::from_value::<ConfirmArguments>(arguments).map(|a| {
                MatrixRequirementsContextInvocation::Confirm(ConfirmMatrixRequirementsContext {
                    request_id: a.request_id,
                    locator: a.locator.into(),
                    proposal_revision: a.proposal_revision,
                    proposal_digest: a.proposal_digest,
                    owner_response_ref: a.owner_response_ref,
                })
            })
        }
        "matrix_context_effective_get" => serde_json::from_value::<GetArguments>(arguments)
            .map(|a| MatrixRequirementsContextInvocation::Get(a.locator.into())),
        _ => return Err(Error::InvalidArguments),
    }
    .map_err(Error::invalid_arguments_from)
}

pub(crate) fn proposal_output(record: tect_application::StoredMatrixRequirementsProposal) -> Value {
    json!({"request":{"request_id":record.request.request_id,"locator":Locator::from(&record.request.locator),"expected_context_revision":record.request.expected_context_revision,"patches":record.request.patches},"proposal":record.proposal,"recorded_at_epoch_seconds":record.recorded_at_epoch_seconds})
}
pub(crate) fn confirmation_output(
    record: tect_application::StoredMatrixRequirementsConfirmation,
) -> Value {
    json!({"request":{"request_id":record.request.request_id,"locator":Locator::from(&record.request.locator),"proposal_revision":record.request.proposal_revision,"proposal_digest":record.request.proposal_digest,"owner_response_ref":record.request.owner_response_ref},"confirmation":record.confirmation,"recorded_at_epoch_seconds":record.recorded_at_epoch_seconds})
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn caller_authority_and_operating_facts_are_rejected() {
        let id = "00000000-0000-4000-8000-000000000001";
        let valid = json!({"request_id":id,"locator":{"level":"program","program_id":id},"expected_context_revision":0,"patches":[{"operation":"set","value":{"kind":"mode","value":"mvp"}}]});
        assert!(parse("matrix_context_propose", valid.clone()).is_ok());
        for field in [
            "human_confirmed",
            "principal_id",
            "recorder",
            "operating_values",
        ] {
            let mut bad = valid.clone();
            bad[field] = json!(true);
            assert!(parse("matrix_context_propose", bad).is_err());
        }
        let mut bad = valid;
        let mut nested = bad.clone();
        nested["patches"][0]["value"] =
            json!({"kind":"intent","value":{"kind":"production_hotfix","human_confirmed":true}});
        assert!(parse("matrix_context_propose", nested).is_err());
        bad["patches"][0]["value"] = json!({"kind":"verified_demand","value":2});
        assert!(parse("matrix_context_propose", bad).is_err());
    }
    #[test]
    fn effective_read_accepts_only_locator() {
        assert!(parse("matrix_context_effective_get", json!({"locator":{"level":"opened_slice","slice_id":"00000000-0000-4000-8000-000000000001"},"freeze":true})).is_err());
    }

    #[test]
    fn declaration_text_accepts_256_bytes_and_rejects_257() {
        let id = "00000000-0000-4000-8000-000000000001";
        let valid = json!({"request_id":id,"locator":{"level":"program","program_id":id},"expected_context_revision":0,"patches":[{"operation":"set","value":{"kind":"urgency","value":"normal"}}]});
        for (kind, nested) in [
            ("intent", true),
            ("urgency", false),
            ("promised_behavior", false),
            ("promised_proof", false),
        ] {
            for length in [256, 257] {
                let text = "x".repeat(length);
                let mut arguments = valid.clone();
                arguments["patches"][0]["value"] = if nested {
                    json!({"kind":kind,"value":{"kind":"other","description":text}})
                } else {
                    json!({"kind":kind,"value":text})
                };
                assert_eq!(
                    parse("matrix_context_propose", arguments).is_ok(),
                    length == 256,
                    "unexpected parse result for {kind} at {length} bytes"
                );
            }
        }

        let confirmation = json!({"request_id":id,"locator":{"level":"program","program_id":id},"proposal_revision":1,"proposal_digest":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","owner_response_ref":"response"});
        for length in [256, 257] {
            let mut arguments = confirmation.clone();
            arguments["owner_response_ref"] = json!("x".repeat(length));
            assert_eq!(
                parse("matrix_context_confirm", arguments).is_ok(),
                length == 256,
                "unexpected owner response parse result at {length} bytes"
            );
        }
    }
}
