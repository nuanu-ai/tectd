use crate::{Error, PipelineDeliveryMode, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PipelineKind {
    LightweightTddDevelopment,
    FullDesignToExecution,
    DebugRootCause,
    OperationalPreparation,
    OperationalExecution,
    Research,
    DeepBrainstorming,
    ResearchToDurableKnowledge,
    CustomProcedureCapture,
    PromoteToDurableKnowledge,
}

impl PipelineKind {
    pub const HISTORICAL_SLICE_RUN_KINDS: [Self; 7] = [
        Self::LightweightTddDevelopment,
        Self::FullDesignToExecution,
        Self::DebugRootCause,
        Self::OperationalPreparation,
        Self::OperationalExecution,
        Self::ResearchToDurableKnowledge,
        Self::CustomProcedureCapture,
    ];
    pub const CURRENT_SLICE_RUN_KINDS: [Self; 8] = [
        Self::LightweightTddDevelopment,
        Self::FullDesignToExecution,
        Self::DebugRootCause,
        Self::OperationalPreparation,
        Self::OperationalExecution,
        Self::Research,
        Self::DeepBrainstorming,
        Self::CustomProcedureCapture,
    ];
    pub const SLICE_RUN_KINDS: [Self; 9] = [
        Self::LightweightTddDevelopment,
        Self::FullDesignToExecution,
        Self::DebugRootCause,
        Self::OperationalPreparation,
        Self::OperationalExecution,
        Self::Research,
        Self::DeepBrainstorming,
        Self::CustomProcedureCapture,
        Self::ResearchToDurableKnowledge,
    ];
    pub const ALL: [Self; 10] = [
        Self::LightweightTddDevelopment,
        Self::FullDesignToExecution,
        Self::DebugRootCause,
        Self::OperationalPreparation,
        Self::OperationalExecution,
        Self::Research,
        Self::DeepBrainstorming,
        Self::ResearchToDurableKnowledge,
        Self::CustomProcedureCapture,
        Self::PromoteToDurableKnowledge,
    ];
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::LightweightTddDevelopment => "slice.lightweight-tdd-development",
            Self::FullDesignToExecution => "slice.full-design-to-execution",
            Self::DebugRootCause => "slice.debug-root-cause",
            Self::OperationalPreparation => "slice.operational-preparation",
            Self::OperationalExecution => "slice.operational-execution",
            Self::Research => "slice.research",
            Self::DeepBrainstorming => "slice.deep-brainstorming",
            Self::ResearchToDurableKnowledge => "slice.research-to-durable-knowledge",
            Self::CustomProcedureCapture => "slice.custom-procedure-capture",
            Self::PromoteToDurableKnowledge => "slice.promote-to-durable-knowledge",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PipelineExecutionOwner {
    #[default]
    SlicePipelineRun,
    KnowledgeChange,
}

impl PipelineExecutionOwner {
    pub const fn is_slice_pipeline_run(&self) -> bool {
        matches!(self, Self::SlicePipelineRun)
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
    #[serde(default)]
    pub executable: bool,
    #[serde(default)]
    pub default_delivery_mode: Option<PipelineDeliveryMode>,
    #[serde(default)]
    pub allowed_delivery_modes: Vec<PipelineDeliveryMode>,
    #[serde(
        default,
        skip_serializing_if = "PipelineExecutionOwner::is_slice_pipeline_run"
    )]
    pub execution_owner: PipelineExecutionOwner,
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
        let expected_kinds = match self.entries.len() {
            7 => PipelineKind::HISTORICAL_SLICE_RUN_KINDS
                .into_iter()
                .collect::<BTreeSet<_>>(),
            8 => PipelineKind::HISTORICAL_SLICE_RUN_KINDS
                .into_iter()
                .chain([PipelineKind::PromoteToDurableKnowledge])
                .collect(),
            9 => PipelineKind::CURRENT_SLICE_RUN_KINDS
                .into_iter()
                .chain([PipelineKind::PromoteToDurableKnowledge])
                .collect(),
            _ => return Err(Error::InvalidArguments),
        };
        if self.revision.trim().is_empty() || self.digest.trim().is_empty() {
            return Err(Error::InvalidArguments);
        }
        let kinds = self.entries.iter().map(|e| e.kind).collect::<BTreeSet<_>>();
        if kinds.len() != self.entries.len()
            || kinds != expected_kinds
            || self.entries.iter().any(|entry| {
                (entry.kind == PipelineKind::PromoteToDurableKnowledge)
                    != (entry.execution_owner == PipelineExecutionOwner::KnowledgeChange)
            })
            || self.entries.iter().any(|e| {
                e.description.trim().is_empty()
                    || !matches!(e.implementation_status.as_str(), "stub" | "executable")
                    || !matches!(e.description_status.as_str(), "provisional" | "refined")
                    || e.executable != (e.implementation_status == "executable")
                    || e.executable != (e.description_status == "refined")
                    || e.refinement_required == e.executable
                    || e.executable
                        && (e.default_delivery_mode.is_none()
                            || e.allowed_delivery_modes.is_empty()
                            || !e
                                .allowed_delivery_modes
                                .contains(e.default_delivery_mode.as_ref().expect("checked")))
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

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(kind: PipelineKind) -> PipelineCatalogueEntry {
        PipelineCatalogueEntry {
            kind,
            description: "description".into(),
            implementation_status: "executable".into(),
            description_status: "refined".into(),
            refinement_required: false,
            choose_when: "choose".into(),
            do_not_choose_when: "avoid".into(),
            expected_result: "result".into(),
            executable: true,
            default_delivery_mode: Some(PipelineDeliveryMode::Phasewise),
            allowed_delivery_modes: vec![
                PipelineDeliveryMode::Phasewise,
                PipelineDeliveryMode::Whole,
            ],
            execution_owner: if kind == PipelineKind::PromoteToDurableKnowledge {
                PipelineExecutionOwner::KnowledgeChange
            } else {
                PipelineExecutionOwner::SlicePipelineRun
            },
        }
    }

    fn snapshot(kinds: impl IntoIterator<Item = PipelineKind>) -> PipelineCatalogueSnapshot {
        PipelineCatalogueSnapshot {
            revision: "test".into(),
            digest: "digest".into(),
            entries: kinds.into_iter().map(entry).collect(),
        }
    }

    #[test]
    fn all_known_kinds_roundtrip_including_historical_research() {
        assert_eq!(PipelineKind::ALL.len(), 10);
        assert_eq!(PipelineKind::SLICE_RUN_KINDS.len(), 9);
        assert_eq!(PipelineKind::CURRENT_SLICE_RUN_KINDS.len(), 8);
        for kind in PipelineKind::ALL {
            let encoded = serde_json::to_string(&kind).unwrap();
            assert_eq!(
                serde_json::from_str::<PipelineKind>(&encoded).unwrap(),
                kind
            );
        }
        assert!(PipelineKind::ALL.contains(&PipelineKind::ResearchToDurableKnowledge));
        assert!(
            !PipelineKind::CURRENT_SLICE_RUN_KINDS
                .contains(&PipelineKind::ResearchToDurableKnowledge)
        );
    }

    #[test]
    fn catalogue_validation_accepts_only_exact_historical_and_current_sets() {
        let historical = snapshot(PipelineKind::HISTORICAL_SLICE_RUN_KINDS);
        historical.validate().unwrap();

        let historical_with_promotion = snapshot(
            PipelineKind::HISTORICAL_SLICE_RUN_KINDS
                .into_iter()
                .chain([PipelineKind::PromoteToDurableKnowledge]),
        );
        historical_with_promotion.validate().unwrap();

        let current = snapshot(
            PipelineKind::CURRENT_SLICE_RUN_KINDS
                .into_iter()
                .chain([PipelineKind::PromoteToDurableKnowledge]),
        );
        current.validate().unwrap();

        let wrong_seven = snapshot(
            PipelineKind::HISTORICAL_SLICE_RUN_KINDS
                .into_iter()
                .take(6)
                .chain([PipelineKind::Research]),
        );
        assert_eq!(wrong_seven.validate(), Err(Error::InvalidArguments));

        let wrong_eight = snapshot(PipelineKind::CURRENT_SLICE_RUN_KINDS);
        assert_eq!(wrong_eight.validate(), Err(Error::InvalidArguments));

        let wrong_nine = snapshot(PipelineKind::SLICE_RUN_KINDS);
        assert_eq!(wrong_nine.validate(), Err(Error::InvalidArguments));
    }
}
