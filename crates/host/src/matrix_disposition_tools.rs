use serde::Deserialize;
use serde_json::{Value, json};
use std::future::Future;
use tect_application::{MatrixDispositionRecord, RecordMatrixDisposition};
use tect_domain::{Error, MatrixDispositionBasis, MatrixDispositionDecision, Result};
use uuid::Uuid;

const MAX_REQUEST_BYTES: usize = 16 * 1024;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GetArguments {
    task_id: Uuid,
    request_id: Uuid,
}

pub(crate) enum MatrixDispositionInvocation {
    Record(RecordMatrixDisposition),
    Get { task_id: Uuid, request_id: Uuid },
}

pub(crate) fn parse(name: &str, arguments: Value) -> Result<MatrixDispositionInvocation> {
    match name {
        "record_matrix_disposition" => {
            if serde_json::to_vec(&arguments)
                .map_err(Error::invalid_arguments_from)?
                .len()
                > MAX_REQUEST_BYTES
            {
                return Err(Error::RequestTooLarge);
            }
            let request: RecordMatrixDisposition =
                serde_json::from_value(arguments).map_err(Error::invalid_arguments_from)?;
            request.validate()?;
            Ok(MatrixDispositionInvocation::Record(request))
        }
        "get_matrix_disposition" => {
            let args: GetArguments =
                serde_json::from_value(arguments).map_err(Error::invalid_arguments_from)?;
            if args.task_id.is_nil() || args.request_id.is_nil() {
                return Err(Error::InvalidArguments);
            }
            Ok(MatrixDispositionInvocation::Get {
                task_id: args.task_id,
                request_id: args.request_id,
            })
        }
        _ => Err(Error::InvalidArguments),
    }
}

pub(crate) fn receipt(record: MatrixDispositionRecord) -> Value {
    let request = record.request;
    json!({
        "disposition_id": record.disposition_id,
        "request_id": request.request_id,
        "task_id": request.task_id,
        "task_revision": request.expected_task_revision,
        "input_digest": request.expected_input_digest,
        "choice_set_digest": request.expected_choice_set_digest,
        "opportunity_id": request.opportunity_id,
        "basis": request.basis,
        "advice_id": request.advice_id,
        "advice_digest": request.advice_digest,
        "decision": request.decision,
        "recorded_by_principal_id": record.recorded_by_principal_id,
        "recorded_by_session_id": record.recorded_by_session_id,
        "material_digest": record.material_digest,
    })
}

/// Reserve the largest allowed escaped decision before the write. All other
/// response fields have fixed-width UUID, digest or enum representations.
pub(crate) fn guard_record_output(
    request: &RecordMatrixDisposition,
    capacity: usize,
) -> Result<()> {
    for decision in [
        MatrixDispositionDecision::Selected {
            selected_choice_id: "\u{0001}".repeat(4096),
        },
        MatrixDispositionDecision::Blocked {
            blocked_reason: "\u{0001}".repeat(4096),
        },
    ] {
        let projected = MatrixDispositionRecord {
            disposition_id: Uuid::nil(),
            request: RecordMatrixDisposition {
                decision,
                basis: MatrixDispositionBasis::AfterAdvice,
                advice_id: Some(Uuid::nil()),
                advice_digest: Some("0".repeat(64)),
                expected_choice_set_digest: Some("0".repeat(64)),
                ..request.clone()
            },
            recorded_by_principal_id: Uuid::nil(),
            recorded_by_session_id: Uuid::nil(),
            material_digest: "0".repeat(64),
        };
        let response = crate::responses::with_actions(receipt(projected), Vec::new(), None);
        if crate::responses::encoded_len(&response)? > capacity {
            return Err(Error::RequestTooLarge);
        }
    }
    Ok(())
}

pub(crate) async fn guarded_record<F, Fut>(
    request: &RecordMatrixDisposition,
    capacity: usize,
    send: F,
) -> Result<MatrixDispositionRecord>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = Result<MatrixDispositionRecord>>,
{
    guard_record_output(request, capacity)?;
    send().await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    fn valid() -> Value {
        json!({
            "request_id": Uuid::new_v4(),
            "task_id": Uuid::new_v4(),
            "expected_task_revision": 1,
            "expected_input_digest": "a".repeat(64),
            "expected_choice_set_digest": "b".repeat(64),
            "opportunity_id": Uuid::new_v4(),
            "basis": "no_call",
            "advice_id": null,
            "advice_digest": null,
            "decision": {"outcome":"selected","selected_choice_id":"choice-a"}
        })
    }

    #[test]
    fn record_requires_explicit_bound_decision_and_no_actor_fields() {
        assert!(matches!(
            parse("record_matrix_disposition", valid()),
            Ok(MatrixDispositionInvocation::Record(_))
        ));
        for field in [
            "recorded_by_principal_id",
            "recorded_by_session_id",
            "raw_jev_payload",
        ] {
            let mut invalid = valid();
            invalid[field] = json!(Uuid::new_v4());
            assert!(
                parse("record_matrix_disposition", invalid).is_err(),
                "{field}"
            );
        }
        let mut invalid = valid();
        invalid["decision"] = json!({"outcome":"selected","selected_choice_id":""});
        assert!(parse("record_matrix_disposition", invalid).is_err());
        let mut invalid = valid();
        invalid["request_id"] = json!(Uuid::nil());
        assert!(parse("record_matrix_disposition", invalid).is_err());
        assert!(
            parse(
                "record_matrix_disposition",
                json!({"decision":{"outcome":"selected"}})
            )
            .is_err()
        );
        let mut blocked = valid();
        blocked["expected_choice_set_digest"] = Value::Null;
        blocked["decision"] = json!({"outcome":"blocked","blocked_reason":"Evidence unresolved"});
        assert!(matches!(
            parse("record_matrix_disposition", blocked),
            Ok(MatrixDispositionInvocation::Record(_))
        ));
        let mut invalid = valid();
        invalid["basis"] = json!("after_advice");
        assert!(parse("record_matrix_disposition", invalid).is_err());
        let mut invalid = valid();
        invalid["decision"]["selected_choice_id"] = json!("x".repeat(MAX_REQUEST_BYTES));
        assert!(matches!(
            parse("record_matrix_disposition", invalid),
            Err(Error::RequestTooLarge)
        ));
    }

    #[test]
    fn read_accepts_only_non_nil_request_id() {
        assert!(matches!(
            parse(
                "get_matrix_disposition",
                json!({"task_id":Uuid::new_v4(),"request_id":Uuid::new_v4()})
            ),
            Ok(MatrixDispositionInvocation::Get { .. })
        ));
        for invalid in [
            json!({"task_id":Uuid::new_v4(),"request_id":Uuid::nil()}),
            json!({"task_id":Uuid::nil(),"request_id":Uuid::new_v4()}),
            json!({"task_id":Uuid::new_v4(),"request_id":Uuid::new_v4(),"actor_id":Uuid::new_v4()}),
        ] {
            assert!(parse("get_matrix_disposition", invalid).is_err());
        }
    }

    #[tokio::test]
    async fn capacity_guard_precedes_persistence() {
        let MatrixDispositionInvocation::Record(request) =
            parse("record_matrix_disposition", valid()).unwrap()
        else {
            unreachable!()
        };
        let called = Cell::new(false);
        let result = guarded_record(&request, 1, || {
            called.set(true);
            async { Err(Error::TransportUnavailable) }
        })
        .await;
        assert!(matches!(result, Err(Error::RequestTooLarge)));
        assert!(!called.get());
    }

    #[test]
    fn receipt_is_typed_and_contains_no_raw_provider_payload() {
        let MatrixDispositionInvocation::Record(request) =
            parse("record_matrix_disposition", valid()).unwrap()
        else {
            unreachable!()
        };
        let record = MatrixDispositionRecord {
            disposition_id: Uuid::new_v4(),
            request,
            recorded_by_principal_id: Uuid::new_v4(),
            recorded_by_session_id: Uuid::new_v4(),
            material_digest: "c".repeat(64),
        };
        let response = receipt(record);
        assert_eq!(response["decision"]["outcome"], "selected");
        assert!(response["recorded_by_principal_id"].is_string());
        assert!(response["material_digest"].is_string());
        assert!(response.get("raw_jev_payload").is_none());
    }
}
