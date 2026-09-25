use serde::Deserialize;
use serde_json::{Value, json};
use tect_application::{
    PipelineOpenEffectAttestation, PipelineOpenEffectMaterial, PipelineOpenEffectVerdict,
    VerifyPipelineOpenEffect,
};
use tect_domain::{Error, Result};
use uuid::Uuid;

pub(crate) enum PipelineOpenEffectInvocation {
    Get {
        slice_id: Uuid,
        open_request_id: Uuid,
    },
    Verify(VerifyPipelineOpenEffect),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GetArguments {
    slice_id: Uuid,
    open_request_id: Uuid,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct VerifyArguments {
    request_id: Uuid,
    slice_id: Uuid,
    open_request_id: Uuid,
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

pub(crate) fn parse(name: &str, arguments: Value) -> Result<PipelineOpenEffectInvocation> {
    match name {
        "get_pipeline_open_effect" => {
            let args: GetArguments =
                serde_json::from_value(arguments).map_err(Error::invalid_arguments_from)?;
            if args.slice_id.is_nil() || args.open_request_id.is_nil() {
                return Err(Error::InvalidArguments);
            }
            Ok(PipelineOpenEffectInvocation::Get {
                slice_id: args.slice_id,
                open_request_id: args.open_request_id,
            })
        }
        "verify_pipeline_open_effect" => {
            let args: VerifyArguments =
                serde_json::from_value(arguments).map_err(Error::invalid_arguments_from)?;
            let request = VerifyPipelineOpenEffect {
                request_id: args.request_id,
                slice_id: args.slice_id,
                open_request_id: args.open_request_id,
                expected_effect_digest: args.expected_effect_digest,
                verdict: match args.verdict {
                    VerdictArgument::Matches => PipelineOpenEffectVerdict::Matches,
                    VerdictArgument::Rejects => PipelineOpenEffectVerdict::Rejects,
                },
                summary: args.summary,
            };
            request.validate()?;
            Ok(PipelineOpenEffectInvocation::Verify(request))
        }
        _ => Err(Error::InvalidArguments),
    }
}

pub(crate) fn read(
    material: PipelineOpenEffectMaterial,
    effect_digest: String,
    verifier_principal_id: Uuid,
    verifier_session_id: Uuid,
) -> Value {
    json!({ "material": material, "effect_digest": effect_digest,
        "verifier_principal_id": verifier_principal_id, "verifier_session_id": verifier_session_id })
}

pub(crate) fn receipt(value: PipelineOpenEffectAttestation) -> Value {
    json!({ "request_id": value.request_id, "slice_id": value.slice_id,
        "open_request_id": value.open_request_id, "effect_digest": value.effect_digest,
        "verifier_principal_id": value.verifier_principal_id,
        "verifier_session_id": value.verifier_session_id,
        "verdict": verdict_name(value.verdict), "summary": value.summary })
}

fn verdict_name(value: PipelineOpenEffectVerdict) -> &'static str {
    match value {
        PipelineOpenEffectVerdict::Matches => "matches",
        PipelineOpenEffectVerdict::Rejects => "rejects",
    }
}

pub(crate) fn guard_verify_output(
    request: &VerifyPipelineOpenEffect,
    capacity: usize,
) -> Result<()> {
    let projected = json!({ "request_id": request.request_id, "slice_id": request.slice_id,
        "open_request_id": request.open_request_id, "effect_digest": request.expected_effect_digest,
        "verifier_principal_id": Uuid::nil(), "verifier_session_id": Uuid::nil(),
        "verdict": verdict_name(request.verdict), "summary": request.summary });
    let response = crate::responses::with_actions(projected, Vec::new(), None);
    if crate::responses::encoded_len(&response)? > capacity {
        return Err(Error::RequestTooLarge);
    }
    Ok(())
}
