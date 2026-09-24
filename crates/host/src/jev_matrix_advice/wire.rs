use serde::{Deserialize, Deserializer, Serialize};
use tect_application::{MatrixProviderRequest, MatrixTaskRevision, canonical_matrix_input_digest};
use tect_domain::{
    EngineeringChoiceSet, EngineeringMatrixComposition, EngineeringMatrixInput, Error,
    MATRIX_EVALUATION_CONTRACT_VERSION, MATRIX_VERIFIED_EVALUATION_CONTRACT_VERSION,
    MatrixAdviceEligibility, MatrixRanking, MatrixSourceVerificationStatus, Result,
    matrix_evaluation_digest,
};

/// All values bind one accepted task revision and its complete evaluation material.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MatrixRankingBinding {
    pub task_id: String,
    pub task_revision: String,
    pub input_digest: String,
    pub choice_set_id: String,
    pub choice_set_version: u64,
    pub choice_set_digest: String,
    pub evaluation_digest: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verification_digest: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedMatrixRankingRequest {
    pub body: Vec<u8>,
    pub model: String,
    pub binding: MatrixRankingBinding,
    pub eligibility: MatrixAdviceEligibility,
    pub contract: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedMatrixRankingResponse {
    pub ranking: MatrixRanking,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
}

#[derive(Serialize)]
struct RequestBody<'a> {
    model: &'a str,
    state: RequestState<'a>,
    questions: RequestQuestions,
}

#[derive(Serialize)]
struct RequestState<'a> {
    contract: &'static str,
    binding: &'a MatrixRankingBinding,
    input: &'a EngineeringMatrixInput,
    composition: &'a EngineeringMatrixComposition,
    choice_set: &'a EngineeringChoiceSet,
}

#[derive(Serialize)]
struct RequestQuestions {
    ranking: RankingQuestion,
}

#[derive(Serialize)]
struct RankingQuestion {
    #[serde(rename = "type")]
    kind: &'static str,
    instructions: &'static str,
    candidate_ids: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ResponseBody {
    contract: String,
    model: String,
    binding: MatrixRankingBinding,
    ranking: MatrixRanking,
    #[serde(deserialize_with = "deserialize_nullable_usage")]
    usage: Option<TokenUsage>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TokenUsage {
    input_tokens: u64,
    output_tokens: u64,
}

fn deserialize_nullable_usage<'de, D>(
    deserializer: D,
) -> std::result::Result<Option<TokenUsage>, D::Error>
where
    D: Deserializer<'de>,
{
    Option::<TokenUsage>::deserialize(deserializer)
}

/// Requires the accepted stored input and choice set, rechecks their stored
/// digests, and rejects non-rankable sets and mismatched compositions.
pub fn prepare_request(
    model: &str,
    revision: &MatrixTaskRevision,
    composition: &EngineeringMatrixComposition,
    maximum_request_bytes: usize,
) -> Result<PreparedMatrixRankingRequest> {
    prepare_request_inner(model, revision, composition, None, maximum_request_bytes)
}

/// Pure v2 wire preparation. Only the application's verified request can
/// supply the record digest and matching evaluation digest.
pub fn prepare_verified_request(
    model: &str,
    request: &MatrixProviderRequest,
    maximum_request_bytes: usize,
) -> Result<PreparedMatrixRankingRequest> {
    let binding = request.binding();
    let digest = binding
        .verification_digest
        .as_deref()
        .ok_or(Error::InvalidArguments)?;
    if digest.len() != 64
        || !digest
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        || request.composition().source_verification_status
            != MatrixSourceVerificationStatus::IndependentlyVerifiedOwnerReported
    {
        return Err(Error::InvalidArguments);
    }
    let prepared = prepare_request_inner(
        model,
        request.revision(),
        request.composition(),
        Some((digest, binding.evaluation_digest.as_str())),
        maximum_request_bytes,
    )?;
    if prepared.binding.task_id != binding.task_id.to_string()
        || prepared.binding.task_revision != binding.task_revision.to_string()
        || prepared.binding.input_digest != binding.input_digest
        || prepared.binding.choice_set_id != binding.choice_set_id
        || prepared.binding.choice_set_version != binding.choice_set_version
        || prepared.binding.choice_set_digest != binding.choice_set_digest
        || prepared.binding.evaluation_digest != binding.evaluation_digest
    {
        return Err(Error::InputConflict);
    }
    Ok(prepared)
}

fn prepare_request_inner(
    model: &str,
    revision: &MatrixTaskRevision,
    composition: &EngineeringMatrixComposition,
    verified: Option<(&str, &str)>,
    maximum_request_bytes: usize,
) -> Result<PreparedMatrixRankingRequest> {
    if model.trim().is_empty()
        || model.len() > 128
        || model.chars().any(char::is_control)
        || maximum_request_bytes == 0
        || revision.task_id.is_nil()
        || revision.revision < 1
    {
        return Err(Error::InvalidArguments);
    }
    let choice_set = revision
        .choice_set
        .as_ref()
        .ok_or(Error::InvalidArguments)?;
    if choice_set.task_id != revision.task_id.to_string()
        || choice_set.task_revision != revision.revision.to_string()
    {
        return Err(Error::StaleRevision);
    }
    let input_value = serde_json::to_value(&revision.input).map_err(|_| Error::InvalidArguments)?;
    let input_digest = canonical_matrix_input_digest(&input_value)?;
    let choice_set_digest = choice_set.canonical_digest(&revision.input)?;
    if revision.input_digest != input_digest
        || revision.choice_set_digest.as_deref() != Some(choice_set_digest.as_str())
    {
        return Err(Error::InvalidArguments);
    }
    let eligibility = choice_set.validate(&revision.input)?;
    let MatrixAdviceEligibility::EligibleForAdvice { candidate_ids } = &eligibility else {
        return Err(Error::InvalidArguments);
    };
    let evaluation_digest = match verified {
        Some((_, evaluation_digest)) => evaluation_digest.to_owned(),
        None => matrix_evaluation_digest(&revision.input, composition, choice_set)?
            .ok_or(Error::InvalidArguments)?,
    };
    let contract = if verified.is_some() {
        MATRIX_VERIFIED_EVALUATION_CONTRACT_VERSION
    } else {
        MATRIX_EVALUATION_CONTRACT_VERSION
    };
    let binding = MatrixRankingBinding {
        task_id: choice_set.task_id.clone(),
        task_revision: choice_set.task_revision.clone(),
        input_digest,
        choice_set_id: choice_set.choice_set_id.clone(),
        choice_set_version: choice_set.version,
        choice_set_digest,
        evaluation_digest,
        verification_digest: verified.map(|(digest, _)| digest.to_owned()),
    };
    let body = serde_json::to_vec(&RequestBody {
        model,
        state: RequestState {
            contract,
            binding: &binding,
            input: &revision.input,
            composition,
            choice_set,
        },
        questions: RequestQuestions {
            ranking: RankingQuestion {
                kind: "ranking",
                instructions: "Rank every supplied candidate ID exactly once, best first, with the first ID recommended. Abstain with an empty ranking and null recommendation if evidence is insufficient. Use only the supplied facts and alternatives; mandatory cards remain mandatory, and this advice authorizes no action.",
                candidate_ids: candidate_ids.clone(),
            },
        },
    })
    .map_err(|_| Error::InvalidArguments)?;
    if body.len() > maximum_request_bytes {
        return Err(Error::RequestTooLarge);
    }
    Ok(PreparedMatrixRankingRequest {
        body,
        model: model.to_owned(),
        binding,
        eligibility,
        contract,
    })
}

/// Parses one bounded, strict response against the exact prepared binding.
/// The caller retains responsibility for checking that the task revision is
/// still current before any use of the advisory result.
pub fn parse_response(
    bytes: &[u8],
    model: &str,
    prepared: &PreparedMatrixRankingRequest,
    maximum_response_bytes: usize,
) -> Result<ParsedMatrixRankingResponse> {
    if maximum_response_bytes == 0 || bytes.len() > maximum_response_bytes {
        return Err(Error::RequestTooLarge);
    }
    let response: ResponseBody =
        serde_json::from_slice(bytes).map_err(|_| Error::InvalidArguments)?;
    if response.contract != prepared.contract
        || response.model != model
        || response.model != prepared.model
        || response.binding != prepared.binding
    {
        return Err(Error::InvalidArguments);
    }
    response.ranking.validate(&prepared.eligibility)?;
    let (input_tokens, output_tokens) = match response.usage {
        Some(usage) => (Some(usage.input_tokens), Some(usage.output_tokens)),
        None => (None, None),
    };
    Ok(ParsedMatrixRankingResponse {
        ranking: response.ranking,
        input_tokens,
        output_tokens,
    })
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
