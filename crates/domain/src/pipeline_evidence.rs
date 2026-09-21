use crate::Result;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const MAX_EVIDENCE_ARTIFACT_FORMAT_BYTES: usize = 128;
pub const MAX_EVIDENCE_ARTIFACT_PROVENANCE_BYTES: usize = 4096;
pub const MAX_EVIDENCE_ARTIFACT_TARGET_BYTES: usize = 4096;
pub const MAX_EVIDENCE_ARTIFACT_PAGE_BYTES: u32 = 64 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PipelineEvidenceArtifactReadiness {
    Uploading,
    Ready,
    Rejected,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PipelineEvidenceArtifact {
    pub artifact_id: Uuid,
    pub digest: String,
    pub size: i64,
    pub format: String,
    pub provenance: String,
    pub target: String,
    pub revision: i64,
    pub readiness: PipelineEvidenceArtifactReadiness,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PipelineEvidenceArtifactRef {
    pub artifact_id: Uuid,
    pub revision: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegisterPipelineEvidenceArtifact {
    pub request_id: Uuid,
    pub digest: String,
    pub size: i64,
    pub format: String,
    pub provenance: String,
    pub target: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FinalizePipelineEvidenceArtifact {
    pub request_id: Uuid,
    pub artifact_id: Uuid,
    pub revision: i64,
    pub body: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReadPipelineEvidenceArtifact {
    pub artifact_id: Uuid,
    pub revision: i64,
    #[serde(default)]
    pub offset: u32,
    #[serde(default = "default_evidence_page_limit")]
    pub limit: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PipelineEvidenceArtifactPage {
    pub artifact: PipelineEvidenceArtifact,
    pub offset: u32,
    pub limit: u32,
    pub fragment: String,
    pub complete: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_offset: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PipelineEvidenceArtifactOutcome {
    pub artifact: PipelineEvidenceArtifact,
    pub replay: bool,
}

fn default_evidence_page_limit() -> u32 {
    MAX_EVIDENCE_ARTIFACT_PAGE_BYTES
}

impl RegisterPipelineEvidenceArtifact {
    pub fn validate(&self) -> Result<()> {
        if self.request_id.is_nil()
            || self.digest.len() != 64
            || !self.digest.bytes().all(|b| b.is_ascii_hexdigit())
            || self.size < 0
            || self.format.trim().is_empty()
            || self.format.len() > MAX_EVIDENCE_ARTIFACT_FORMAT_BYTES
            || self.provenance.trim().is_empty()
            || self.provenance.len() > MAX_EVIDENCE_ARTIFACT_PROVENANCE_BYTES
            || self.target.trim().is_empty()
            || self.target.len() > MAX_EVIDENCE_ARTIFACT_TARGET_BYTES
        {
            return Err(crate::Error::InvalidArguments);
        }
        Ok(())
    }
}

impl FinalizePipelineEvidenceArtifact {
    pub fn validate(&self) -> Result<()> {
        if self.request_id.is_nil()
            || self.artifact_id.is_nil()
            || self.revision < 1
            || self.body.len() > crate::MAX_PIPELINE_OUTPUT_BYTES
        {
            Err(crate::Error::InvalidArguments)
        } else {
            Ok(())
        }
    }
}

impl ReadPipelineEvidenceArtifact {
    pub fn validate(&self) -> Result<()> {
        if self.artifact_id.is_nil()
            || self.revision < 1
            || self.limit == 0
            || self.limit > MAX_EVIDENCE_ARTIFACT_PAGE_BYTES
        {
            Err(crate::Error::InvalidArguments)
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_requires_sha256_and_bounded_metadata() {
        let request = RegisterPipelineEvidenceArtifact {
            request_id: Uuid::new_v4(),
            digest: "x".into(),
            size: 1,
            format: "text/plain".into(),
            provenance: "observed".into(),
            target: "slice".into(),
        };
        assert_eq!(request.validate(), Err(crate::Error::InvalidArguments));
    }

    #[test]
    fn read_rejects_unbounded_pages() {
        let request = ReadPipelineEvidenceArtifact {
            artifact_id: Uuid::new_v4(),
            revision: 1,
            offset: 0,
            limit: MAX_EVIDENCE_ARTIFACT_PAGE_BYTES + 1,
        };
        assert_eq!(request.validate(), Err(crate::Error::InvalidArguments));
    }
}
