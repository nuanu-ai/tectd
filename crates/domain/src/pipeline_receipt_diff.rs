//! Complete, deterministic differences between pinned and submitted receipt triples.
//! Client triples describe submitted references; they are not actor verification.
use crate::PipelineSkillReadReceipt;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReceiptDuplicate {
    #[serde(flatten)]
    pub receipt: PipelineSkillReadReceipt,
    pub repeat_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FullReceiptDiff {
    pub expected_unique_count: usize,
    pub submitted_count: usize,
    pub submitted_unique_count: usize,
    pub missing: Vec<PipelineSkillReadReceipt>,
    pub unexpected: Vec<PipelineSkillReadReceipt>,
    pub duplicates: Vec<ReceiptDuplicate>,
}

impl FullReceiptDiff {
    pub fn between(expected: &[(&str, &str, &str)], submitted: &[(&str, &str, &str)]) -> Self {
        let expected_set = expected.iter().copied().collect::<BTreeSet<_>>();
        let mut counts = BTreeMap::new();
        for tuple in submitted {
            *counts.entry(*tuple).or_insert(0usize) += 1;
        }
        let submitted_set = counts.keys().copied().collect::<BTreeSet<_>>();
        let receipt =
            |(instruction_id, version, digest): (&str, &str, &str)| PipelineSkillReadReceipt {
                instruction_id: instruction_id.into(),
                version: version.into(),
                digest: digest.into(),
            };
        Self {
            expected_unique_count: expected_set.len(),
            submitted_count: submitted.len(),
            submitted_unique_count: submitted_set.len(),
            missing: expected_set
                .difference(&submitted_set)
                .copied()
                .map(receipt)
                .collect(),
            unexpected: submitted_set
                .difference(&expected_set)
                .copied()
                .map(receipt)
                .collect(),
            duplicates: counts
                .into_iter()
                .filter(|(_, count)| *count > 1)
                .map(|(tuple, repeat_count)| ReceiptDuplicate {
                    receipt: receipt(tuple),
                    repeat_count,
                })
                .collect(),
        }
    }

    pub fn diagnostic_sections(&self) -> Result<(String, String), serde_json::Error> {
        Ok((
            serde_json::to_string(
                &serde_json::json!({"expected_unique_count":self.expected_unique_count,"missing":self.missing}),
            )?,
            serde_json::to_string(
                &serde_json::json!({"submitted_count":self.submitted_count,"submitted_unique_count":self.submitted_unique_count,"unexpected":self.unexpected,"duplicates":self.duplicates}),
            )?,
        ))
    }
}

#[derive(Serialize)]
struct CanonicalReceiptMultiset {
    discriminator: &'static str,
    receipt_kind: crate::PipelineReceiptKind,
    entries: Vec<ReceiptMultisetEntry>,
}
#[derive(Serialize)]
struct ReceiptMultisetEntry {
    instruction_id: String,
    version: String,
    digest: String,
    count: u64,
}

pub fn pipeline_receipt_multiset_digest(
    receipt_kind: crate::PipelineReceiptKind,
    submitted: &[PipelineSkillReadReceipt],
    digest_port: &dyn crate::PipelineDefinitionDigestPort,
) -> crate::Result<String> {
    let mut counts = BTreeMap::new();
    for receipt in submitted {
        *counts
            .entry((&receipt.instruction_id, &receipt.version, &receipt.digest))
            .or_insert(0u64) += 1;
    }
    let material = CanonicalReceiptMultiset {
        discriminator: "tectd.receipt-multiset.v1",
        receipt_kind,
        entries: counts
            .into_iter()
            .map(
                |((instruction_id, version, digest), count)| ReceiptMultisetEntry {
                    instruction_id: instruction_id.clone(),
                    version: version.clone(),
                    digest: digest.clone(),
                    count,
                },
            )
            .collect(),
    };
    let bytes = serde_json::to_vec(&material).map_err(|_| crate::Error::InternalInvariant)?;
    Ok(digest_port
        .sha256(&bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

#[cfg(test)]
mod tests;
