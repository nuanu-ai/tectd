use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PipelineKind {
    LightweightTddDevelopment,
    FullDesignToExecution,
    DebugRootCause,
    OperationalPreparation,
    OperationalExecution,
    ResearchToDurableKnowledge,
    CustomProcedureCapture,
}

impl PipelineKind {
    pub const ALL: [Self; 7] = [
        Self::LightweightTddDevelopment,
        Self::FullDesignToExecution,
        Self::DebugRootCause,
        Self::OperationalPreparation,
        Self::OperationalExecution,
        Self::ResearchToDurableKnowledge,
        Self::CustomProcedureCapture,
    ];
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::LightweightTddDevelopment => "slice.lightweight-tdd-development",
            Self::FullDesignToExecution => "slice.full-design-to-execution",
            Self::DebugRootCause => "slice.debug-root-cause",
            Self::OperationalPreparation => "slice.operational-preparation",
            Self::OperationalExecution => "slice.operational-execution",
            Self::ResearchToDurableKnowledge => "slice.research-to-durable-knowledge",
            Self::CustomProcedureCapture => "slice.custom-procedure-capture",
        }
    }
}
impl Serialize for PipelineKind {
    fn serialize<S: serde::Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        s.serialize_str(self.as_str())
    }
}
impl<'de> Deserialize<'de> for PipelineKind {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        let v = String::deserialize(d)?;
        Self::ALL
            .into_iter()
            .find(|k| k.as_str() == v)
            .ok_or_else(|| serde::de::Error::custom("unknown Slice pipeline"))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PipelineCatalogueEntry {
    pub kind: PipelineKind,
    pub description: String,
    pub implementation_status: String,
    pub description_status: String,
    pub refinement_required: bool,
    pub choose_when: String,
    pub do_not_choose_when: String,
    pub expected_result: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PipelineCatalogueSnapshot {
    pub revision: String,
    pub digest: String,
    pub entries: Vec<PipelineCatalogueEntry>,
}
impl PipelineCatalogueSnapshot {
    pub fn validate(&self) -> Result<()> {
        if self.revision.trim().is_empty()
            || self.digest.trim().is_empty()
            || self.entries.len() != 7
        {
            return Err(Error::InvalidArguments);
        }
        let kinds = self.entries.iter().map(|e| e.kind).collect::<BTreeSet<_>>();
        if kinds.len() != 7
            || self.entries.iter().any(|e| {
                e.description.trim().is_empty()
                    || e.implementation_status != "stub"
                    || e.description_status != "provisional"
                    || !e.refinement_required
                    || e.choose_when.trim().is_empty()
                    || e.do_not_choose_when.trim().is_empty()
                    || e.expected_result.trim().is_empty()
            })
        {
            Err(Error::InvalidArguments)
        } else {
            Ok(())
        }
    }
}
