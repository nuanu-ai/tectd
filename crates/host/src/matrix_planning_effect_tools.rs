use serde::Deserialize;
use serde_json::{Value, json};
use tect_application::{
    MatrixPlanningEffectAttestation, MatrixPlanningEffectRead, MatrixPlanningEffectVerdict,
    VerifyMatrixPlanningEffect,
};
use tect_domain::{Error, Result};
use uuid::Uuid;

pub(crate) enum MatrixPlanningEffectInvocation {
    Get {
        candidate_set_id: Uuid,
        caller_request_id: Uuid,
    },
    Verify(VerifyMatrixPlanningEffect),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GetArguments {
    candidate_set_id: Uuid,
    caller_request_id: Uuid,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct VerifyArguments {
    request_id: Uuid,
    candidate_set_id: Uuid,
    caller_request_id: Uuid,
    expected_result_revision: i64,
    expected_effect_digest: String,
    verdict: VerdictArgument,
    summary: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum VerdictArgument {
    Matches,
    Rejects,
}

pub(crate) fn parse(name: &str, arguments: Value) -> Result<MatrixPlanningEffectInvocation> {
    match name {
        "get_matrix_planning_effect" => {
            let args: GetArguments =
                serde_json::from_value(arguments).map_err(Error::invalid_arguments_from)?;
            if args.candidate_set_id.is_nil() || args.caller_request_id.is_nil() {
                return Err(Error::InvalidArguments);
            }
            Ok(MatrixPlanningEffectInvocation::Get {
                candidate_set_id: args.candidate_set_id,
                caller_request_id: args.caller_request_id,
            })
        }
        "verify_matrix_planning_effect" => {
            let args: VerifyArguments =
                serde_json::from_value(arguments).map_err(Error::invalid_arguments_from)?;
            let request = VerifyMatrixPlanningEffect {
                request_id: args.request_id,
                candidate_set_id: args.candidate_set_id,
                caller_request_id: args.caller_request_id,
                expected_result_revision: args.expected_result_revision,
                expected_effect_digest: args.expected_effect_digest,
                verdict: match args.verdict {
                    VerdictArgument::Matches => MatrixPlanningEffectVerdict::Matches,
                    VerdictArgument::Rejects => MatrixPlanningEffectVerdict::Rejects,
                },
                summary: args.summary,
            };
            request.validate()?;
            Ok(MatrixPlanningEffectInvocation::Verify(request))
        }
        _ => Err(Error::InvalidArguments),
    }
}

pub(crate) fn read(read: MatrixPlanningEffectRead) -> Value {
    json!({
        "material": read.material,
        "effect_digest": read.effect_digest,
        "verifier_principal_id": read.verifier_principal_id,
        "verifier_session_id": read.verifier_session_id,
    })
}

pub(crate) fn receipt(attestation: MatrixPlanningEffectAttestation) -> Value {
    json!({
        "request_id": attestation.request_id,
        "candidate_set_id": attestation.candidate_set_id,
        "caller_request_id": attestation.caller_request_id,
        "expected_result_revision": attestation.expected_result_revision,
        "effect_digest": attestation.effect_digest,
        "verifier_principal_id": attestation.verifier_principal_id,
        "verifier_session_id": attestation.verifier_session_id,
        "verdict": verdict_name(attestation.verdict),
        "summary": attestation.summary,
    })
}

fn verdict_name(verdict: MatrixPlanningEffectVerdict) -> &'static str {
    match verdict {
        MatrixPlanningEffectVerdict::Matches => "matches",
        MatrixPlanningEffectVerdict::Rejects => "rejects",
    }
}

pub(crate) fn guard_verify_output(
    request: &VerifyMatrixPlanningEffect,
    capacity: usize,
) -> Result<()> {
    let projected = json!({
        "request_id": request.request_id,
        "candidate_set_id": request.candidate_set_id,
        "caller_request_id": request.caller_request_id,
        "expected_result_revision": request.expected_result_revision,
        "effect_digest": request.expected_effect_digest,
        "verifier_principal_id": Uuid::nil(),
        "verifier_session_id": Uuid::nil(),
        "verdict": verdict_name(request.verdict),
        "summary": request.summary,
    });
    let response = crate::responses::with_actions(projected, Vec::new(), None);
    if crate::responses::encoded_len(&response)? > capacity {
        return Err(Error::RequestTooLarge);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strict_get_and_verify_arguments() {
        let get = json!({"candidate_set_id":Uuid::new_v4(),"caller_request_id":Uuid::new_v4()});
        assert!(matches!(
            parse("get_matrix_planning_effect", get.clone()),
            Ok(MatrixPlanningEffectInvocation::Get { .. })
        ));
        let mut extra = get;
        extra["task_id"] = json!(Uuid::new_v4());
        assert!(parse("get_matrix_planning_effect", extra).is_err());

        let verify = json!({
            "request_id":Uuid::new_v4(),"candidate_set_id":Uuid::new_v4(),
            "caller_request_id":Uuid::new_v4(),"expected_result_revision":1,
            "expected_effect_digest":"a".repeat(64),"verdict":"matches","summary":"Checked saved nodes"
        });
        assert!(matches!(
            parse("verify_matrix_planning_effect", verify.clone()),
            Ok(MatrixPlanningEffectInvocation::Verify(_))
        ));
        for (field, value) in [
            ("verdict", json!("matched")),
            ("expected_effect_digest", json!("A".repeat(64))),
            ("summary", json!(" ")),
        ] {
            let mut bad = verify.clone();
            bad[field] = value;
            assert!(
                parse("verify_matrix_planning_effect", bad).is_err(),
                "accepted {field}"
            );
        }
        let mut extra = verify;
        extra["material"] = json!({});
        assert!(parse("verify_matrix_planning_effect", extra).is_err());
    }

    #[test]
    fn small_capacity_rejects_attestation_receipt() {
        let request = match parse(
            "verify_matrix_planning_effect",
            json!({
                "request_id":Uuid::new_v4(),"candidate_set_id":Uuid::new_v4(),
                "caller_request_id":Uuid::new_v4(),"expected_result_revision":1,
                "expected_effect_digest":"a".repeat(64),"verdict":"rejects","summary":"Mismatch"
            }),
        )
        .unwrap()
        {
            MatrixPlanningEffectInvocation::Verify(request) => request,
            _ => unreachable!(),
        };
        assert_eq!(
            guard_verify_output(&request, 1),
            Err(Error::RequestTooLarge)
        );
    }
}
