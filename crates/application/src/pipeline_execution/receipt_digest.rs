use sha2::{Digest, Sha256};

pub(super) struct ReceiptDigest;
impl tect_domain::PipelineDefinitionDigestPort for ReceiptDigest {
    fn sha256(&self, canonical_json: &[u8]) -> [u8; 32] {
        Sha256::digest(canonical_json).into()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use tect_domain::PipelineDefinitionDigestPort;
    #[test]
    fn receipt_digest_adapter_matches_sha256_known_vector() {
        let actual = ReceiptDigest
            .sha256(b"abc")
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        assert_eq!(
            actual,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
    #[test]
    fn receipt_multiset_real_sha_identity_changes_with_kind_multiplicity_and_each_field() {
        use tect_domain::{
            PipelineReceiptKind, PipelineSkillReadReceipt, pipeline_receipt_multiset_digest,
        };
        let original = vec![
            PipelineSkillReadReceipt {
                instruction_id: "i@界".into(),
                version: "v".into(),
                digest: "d".into(),
            },
            PipelineSkillReadReceipt {
                instruction_id: "z".into(),
                version: "v".into(),
                digest: "d".into(),
            },
        ];
        let digest = |kind, values: &[PipelineSkillReadReceipt]| {
            pipeline_receipt_multiset_digest(kind, values, &ReceiptDigest).unwrap()
        };
        let first = digest(PipelineReceiptKind::Skill, &original);
        let mut reordered = original.clone();
        reordered.reverse();
        assert_eq!(first, digest(PipelineReceiptKind::Skill, &reordered));
        assert_ne!(first, digest(PipelineReceiptKind::Resource, &original));
        let mut duplicate = original.clone();
        duplicate.push(original[0].clone());
        assert_ne!(first, digest(PipelineReceiptKind::Skill, &duplicate));
        for field in 0..3 {
            let mut changed = original.clone();
            match field {
                0 => changed[0].instruction_id.push('x'),
                1 => changed[0].version.push('x'),
                _ => changed[0].digest.push('x'),
            };
            assert_ne!(first, digest(PipelineReceiptKind::Skill, &changed));
        }
    }
}
