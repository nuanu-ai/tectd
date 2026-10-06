use crate::{Error, SliceCandidateNode};
use serde_json::Value;
use sha2::{Digest, Sha256};

/// An immutable snapshot and commitment to the whole CURRENT typed Work body.
///
/// This is distinct from the partial `ModelRouteWorkContext.digest`. It does not
/// commit unknown raw JSON fields or establish persisted authenticity or issuer
/// identity. Caller facts remain assertions, not independent authority.
/// It performs no revision, UUID, checkpoint eligibility/currentness,
/// authorization, or policy validation. Future Work Serde changes require an
/// explicit compatibility/version review; do not automatically regenerate goldens.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OrdinaryWorkBodyCommitmentV2 {
    work: SliceCandidateNode,
    canonical_bytes: Vec<u8>,
    digest: String,
}

impl OrdinaryWorkBodyCommitmentV2 {
    pub fn work(&self) -> &SliceCandidateNode {
        &self.work
    }

    pub fn canonical_bytes(&self) -> &[u8] {
        &self.canonical_bytes
    }

    pub fn digest(&self) -> &str {
        &self.digest
    }
}

/// Clone a typed Work, then commit its stored snapshot in the v2 envelope.
/// Decisions are rejected with `Error::InvalidArguments`.
pub fn commit_ordinary_work_body_v2(
    node: &SliceCandidateNode,
) -> crate::Result<OrdinaryWorkBodyCommitmentV2> {
    let work = node.clone();
    if !matches!(&work, SliceCandidateNode::Work { .. }) {
        return Err(Error::InvalidArguments);
    }
    let envelope = serde_json::json!({
        "schema": "ordinary-model-route/work-body/v2",
        "work": serde_json::to_value(&work).map_err(|_| Error::InvalidArguments)?,
    });
    let canonical_bytes = canonical_bytes(envelope)?;
    let digest = format!("{:x}", Sha256::digest(&canonical_bytes));
    Ok(OrdinaryWorkBodyCommitmentV2 {
        work,
        canonical_bytes,
        digest,
    })
}

fn canonical_bytes(value: Value) -> crate::Result<Vec<u8>> {
    fn ordered(value: Value) -> Value {
        match value {
            Value::Object(values) => {
                let sorted: std::collections::BTreeMap<_, _> = values.into_iter().collect();
                Value::Object(
                    sorted
                        .into_iter()
                        .map(|(key, value)| (key, ordered(value)))
                        .collect(),
                )
            }
            Value::Array(values) => Value::Array(values.into_iter().map(ordered).collect()),
            other => other,
        }
    }
    serde_json::to_vec(&ordered(value)).map_err(|_| Error::InvalidArguments)
}

#[cfg(test)]
mod tests;
