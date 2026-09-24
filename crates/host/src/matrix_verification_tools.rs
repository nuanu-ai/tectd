use serde::Deserialize;
use serde_json::{Value, json};
use tect_application::{MatrixEvidenceReference, VerifyMatrixTask};
use tect_domain::{Error, MatrixVerificationRecord, Result};
use uuid::Uuid;

const MAX_EVIDENCE: usize = 1040;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct VerifyArguments {
    task_id: Uuid,
    expected_revision: i64,
    input_digest: String,
    evidence: Vec<EvidenceArguments>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EvidenceArguments {
    fact_path: String,
    evidence_ref: String,
}

pub(crate) fn parse(arguments: Value) -> Result<VerifyMatrixTask> {
    let args: VerifyArguments =
        serde_json::from_value(arguments).map_err(Error::invalid_arguments_from)?;
    if args.task_id.is_nil()
        || args.expected_revision < 1
        || args.input_digest.len() != 64
        || !args
            .input_digest
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
        || args.evidence.len() > MAX_EVIDENCE
        || args.evidence.iter().any(|item| {
            item.fact_path.is_empty()
                || item.fact_path.len() > 512
                || !item.fact_path.starts_with('/')
                || item.evidence_ref.trim().is_empty()
                || item.evidence_ref.len() > 4096
        })
    {
        return Err(Error::InvalidArguments);
    }
    Ok(VerifyMatrixTask {
        task_id: args.task_id,
        expected_revision: args.expected_revision,
        input_digest: args.input_digest,
        evidence: args
            .evidence
            .into_iter()
            .map(|item| MatrixEvidenceReference {
                fact_path: item.fact_path,
                evidence_ref: item.evidence_ref,
            })
            .collect(),
    })
}

pub(crate) fn receipt(record: MatrixVerificationRecord) -> Value {
    // The service returns a sealed record, not the database row ID.
    json!({
        "verification_digest":record.digest,
        "task_id":record.task_id,
        "task_revision":record.task_revision,
        "input_digest":record.input_digest,
        "policy_version":record.policy_version,
        "facts":record.bindings.into_iter().map(|binding| json!({
            "fact_path":binding.fact_path,
            "status":binding.validation_outcome,
            "value_digest":binding.value_digest,
            "content_digest":binding.content_digest,
        })).collect::<Vec<_>>()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verification_accepts_only_reference_fields() {
        let valid = json!({
            "task_id":Uuid::new_v4(), "expected_revision":1,
            "input_digest":"a".repeat(64),
            "evidence":[{"fact_path":"/mode","evidence_ref":"urn:evidence:1"}]
        });
        assert_eq!(parse(valid.clone()).unwrap().evidence.len(), 1);
        for (field, value) in [
            ("verified", json!(true)),
            ("principal", json!(Uuid::new_v4())),
            ("session", json!(Uuid::new_v4())),
            ("outcome", json!("accepted")),
        ] {
            let mut invalid = valid.clone();
            invalid[field] = value;
            assert!(parse(invalid).is_err(), "accepted {field}");
        }
        let mut invalid = valid.clone();
        invalid["evidence"][0]["validation_outcome"] = json!("accepted");
        assert!(parse(invalid).is_err());
    }
}
