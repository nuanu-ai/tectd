use serde::Deserialize;
use serde_json::{Value, json};
use tect_application::{
    PipelinePhaseEffectAttestation, PipelinePhaseEffectMaterial, PipelinePhaseEffectVerdict,
    VerifyPipelinePhaseEffect,
};
use tect_domain::{Error, Result};
use uuid::Uuid;

pub(crate) enum PipelinePhaseEffectInvocation {
    Get { run_id: Uuid, attempt_id: Uuid },
    Verify(VerifyPipelinePhaseEffect),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GetArguments {
    run_id: Uuid,
    attempt_id: Uuid,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct VerifyArguments {
    request_id: Uuid,
    run_id: Uuid,
    attempt_id: Uuid,
    expected_effect_digest: String,
    verdict: VerdictArgument,
    #[serde(default)]
    observation: Option<String>,
    #[serde(default)]
    observed_output_digest: Option<String>,
    summary: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum VerdictArgument {
    Pass,
    Fail,
    Unknown,
}

pub(crate) fn parse(name: &str, arguments: Value) -> Result<PipelinePhaseEffectInvocation> {
    match name {
        "get_pipeline_phase_effect" => {
            let args: GetArguments =
                serde_json::from_value(arguments).map_err(Error::invalid_arguments_from)?;
            if args.run_id.is_nil() || args.attempt_id.is_nil() {
                return Err(Error::InvalidArguments);
            }
            Ok(PipelinePhaseEffectInvocation::Get {
                run_id: args.run_id,
                attempt_id: args.attempt_id,
            })
        }
        "verify_pipeline_phase_effect" => {
            let args: VerifyArguments =
                serde_json::from_value(arguments).map_err(Error::invalid_arguments_from)?;
            let request = VerifyPipelinePhaseEffect {
                request_id: args.request_id,
                run_id: args.run_id,
                attempt_id: args.attempt_id,
                expected_effect_digest: args.expected_effect_digest,
                verdict: match args.verdict {
                    VerdictArgument::Pass => PipelinePhaseEffectVerdict::Pass,
                    VerdictArgument::Fail => PipelinePhaseEffectVerdict::Fail,
                    VerdictArgument::Unknown => PipelinePhaseEffectVerdict::Unknown,
                },
                observation: args.observation,
                observed_output_digest: args.observed_output_digest,
                summary: args.summary,
            };
            request.validate()?;
            Ok(PipelinePhaseEffectInvocation::Verify(request))
        }
        _ => Err(Error::InvalidArguments),
    }
}

pub(crate) fn read(
    material: PipelinePhaseEffectMaterial,
    effect_digest: String,
    principal: Uuid,
    session: Uuid,
) -> Value {
    json!({"material":material,"effect_digest":effect_digest,"verifier_principal_id":principal,"verifier_session_id":session})
}
pub(crate) fn receipt(value: PipelinePhaseEffectAttestation) -> Value {
    json!({"request_id":value.request_id,"run_id":value.run_id,"attempt_id":value.attempt_id,
        "effect_digest":value.effect_digest,"output_digest":value.output_digest,
        "verifier_principal_id":value.verifier_principal_id,"verifier_session_id":value.verifier_session_id,
        "verdict":value.verdict.as_str(),"observation_digest":value.observation_digest,"summary":value.summary})
}
pub(crate) fn guard_verify_output(
    request: &VerifyPipelinePhaseEffect,
    capacity: usize,
) -> Result<()> {
    let projected = json!({"request_id":request.request_id,"run_id":request.run_id,"attempt_id":request.attempt_id,
        "effect_digest":request.expected_effect_digest,"output_digest":request.observed_output_digest,
        "verifier_principal_id":Uuid::nil(),"verifier_session_id":Uuid::nil(),
        "verdict":request.verdict.as_str(),"observation_digest":request.observation.as_ref().map(|_| "0".repeat(64)),
        "summary":request.summary});
    if crate::responses::encoded_len(&crate::responses::with_actions(projected, Vec::new(), None))?
        > capacity
    {
        return Err(Error::RequestTooLarge);
    }
    Ok(())
}
