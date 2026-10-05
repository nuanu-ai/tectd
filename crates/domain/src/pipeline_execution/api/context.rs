use super::*;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PipelineRunContextQuery {
    pub run_id: Uuid,
    #[serde(default)]
    pub view: PipelineRunContextView,
    #[serde(default)]
    pub output_id: Option<Uuid>,
    #[serde(default)]
    pub digest: Option<String>,
    /// Compatibility flag; Current always returns compact snapshot references.
    /// Full definition bodies are read through the pinned snapshot view.
    #[serde(default)]
    pub refresh: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset_bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit_bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub representation_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub definition_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phase_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_revision: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub section: Option<PipelineDetailsSection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receipt_kind: Option<PipelineReceiptKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub submitted_receipts: Option<Vec<PipelineSkillReadReceipt>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub submitted_digest: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PipelineRunContextView {
    #[default]
    Current,
    Output,
    DeliveryReceipt,
    Snapshot,
    PhaseContract,
    Details,
    ReceiptDiff,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PipelineContextResponse {
    Current(Box<PipelineRunContext>),
    Output(Box<PipelinePhaseOutput>),
    DeliveryReceipt(Box<PipelineDeliveryReceipt>),
    Snapshot(Box<PipelineSnapshotRead>),
    PhaseContract(Box<PipelinePhaseContractRead>),
    Details(Box<PipelineDetailsRead>),
    ReceiptDiff(Box<PipelineReceiptDiffRead>),
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PipelineDetailsSection {
    #[default]
    All,
    Inputs,
    Outputs,
    History,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PipelineSnapshotRead {
    pub run_id: Uuid,
    pub definition_digest: String,
    pub definition: PipelineDefinitionSnapshot,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PipelinePhaseContractRead {
    pub run_id: Uuid,
    pub definition_digest: String,
    pub phase: PipelinePhaseDefinition,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PipelineDetailsRead {
    pub run_id: Uuid,
    pub run_revision: i64,
    pub section: PipelineDetailsSection,
    pub data: serde_json::Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PipelineReceiptKind {
    Skill,
    Resource,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PipelineReceiptDiffRead {
    pub run_id: Uuid,
    pub phase_id: String,
    pub definition_version: String,
    pub definition_digest: String,
    pub receipt_kind: PipelineReceiptKind,
    pub submitted_digest: String,
    pub diff: crate::FullReceiptDiff,
}

impl PipelineRunContextQuery {
    /// Called only after the stored context has passed the authenticated copy gate.
    pub fn resolve_pinned_read(
        &self,
        context: &PipelineRunContext,
    ) -> Result<PipelineContextResponse> {
        self.validate()?;
        if self.run_id != context.run.id {
            return Err(Error::NotFound);
        }
        if matches!(
            self.view,
            PipelineRunContextView::Snapshot | PipelineRunContextView::PhaseContract
        ) && self.definition_digest.as_deref() != Some(context.run.definition_digest.as_str())
        {
            return Err(Error::refused_at(
                RefusalCode::MethodVersionUnavailable,
                "PIPELINE-SNAPSHOT-PIN",
                "arguments.params.definition_digest",
                "stored definition digest",
                "definition pin mismatch",
                "read_current_snapshot_reference",
                "definition_digest",
            ));
        }
        match self.view {
            PipelineRunContextView::Snapshot => Ok(PipelineContextResponse::Snapshot(Box::new(
                PipelineSnapshotRead {
                    run_id: context.run.id,
                    definition_digest: context.run.definition_digest.clone(),
                    definition: context.definition.clone(),
                },
            ))),
            PipelineRunContextView::PhaseContract => {
                let phase = context
                    .definition
                    .phases
                    .iter()
                    .find(|phase| Some(phase.id.as_str()) == self.phase_id.as_deref())
                    .ok_or(Error::NotFound)?;
                Ok(PipelineContextResponse::PhaseContract(Box::new(
                    PipelinePhaseContractRead {
                        run_id: context.run.id,
                        definition_digest: context.run.definition_digest.clone(),
                        phase: phase.clone(),
                    },
                )))
            }
            PipelineRunContextView::Details => {
                if self.run_revision != Some(context.run.revision) {
                    return Err(Error::refused_at(
                        RefusalCode::StaleRevision,
                        "PIPELINE-DETAILS-REVISION",
                        "arguments.params.run_revision",
                        context.run.revision.to_string(),
                        "revision pin mismatch",
                        "refresh_pipeline_context",
                        "run_revision",
                    ));
                }
                let section = self.section.unwrap_or_default();
                let mut data = serde_json::Map::new();
                if matches!(
                    section,
                    PipelineDetailsSection::All | PipelineDetailsSection::Inputs
                ) {
                    let ordinal = context.run.current_phase_ordinal.unwrap_or(0);
                    let consumed_outputs: Vec<_> = context.bindings.iter().filter(|binding| binding.phase_ordinal < ordinal && !binding.stale)
                        .map(|binding| serde_json::json!({"phase_id":binding.phase_id,"output_revision":binding.output_revision,"digest":binding.output_digest})).collect();
                    let consumed_inputs: Vec<_> = context.inputs.iter().filter(|input| Some(&input.phase_id) == context.run.current_phase_id.as_ref())
                        .map(|input| serde_json::json!({"input_id":input.id,"sequence":input.sequence,"digest":input.digest})).collect();
                    let consumed_knowledge = context
                        .knowledge
                        .as_ref()
                        .filter(|manifest| !manifest.selected.is_empty())
                        .map(|manifest| (manifest.id, &manifest.digest))
                        .or_else(|| {
                            context
                                .knowledge_resources
                                .as_ref()
                                .filter(|manifest| !manifest.selected.is_empty())
                                .map(|manifest| (manifest.id, &manifest.digest))
                        })
                        .map(|(id, digest)| serde_json::json!({"manifest_id":id,"digest":digest}));
                    let legacy = !context.run.definition_version.starts_with("0.7");
                    data.extend(serde_json::json!({"inputs":context.inputs,"knowledge":context.knowledge,"knowledge_status":context.knowledge_status,
                        "knowledge_resources":context.knowledge_resources,"knowledge_resource_status":context.knowledge_resource_status}).as_object().unwrap().clone());
                    if legacy {
                        data.extend(serde_json::json!({"consumed_outputs":consumed_outputs,"consumed_inputs":consumed_inputs,"consumed_knowledge":consumed_knowledge}).as_object().unwrap().clone());
                    }
                }
                if matches!(
                    section,
                    PipelineDetailsSection::All | PipelineDetailsSection::Outputs
                ) {
                    data.extend(serde_json::json!({"bindings":context.bindings,"outputs":context.outputs,"outputs_complete":context.outputs_complete,
                        "erasure":{"payloads_omitted":!context.outputs_complete}}).as_object().unwrap().clone());
                }
                if matches!(
                    section,
                    PipelineDetailsSection::All | PipelineDetailsSection::History
                ) {
                    data.extend(serde_json::json!({"attempts":context.attempts,"checkpoints":context.checkpoints,"inquiry":context.inquiry,
                        "result":context.result,"source_checkpoint":context.source_checkpoint,"qualification_reason":context.run.qualification_reason,
                        "delivered_phases":context.delivered_phases,"delivery_receipt":context.delivery_receipt}).as_object().unwrap().clone());
                }
                Ok(PipelineContextResponse::Details(Box::new(
                    PipelineDetailsRead {
                        run_id: context.run.id,
                        run_revision: context.run.revision,
                        section,
                        data: serde_json::Value::Object(data),
                    },
                )))
            }
            _ => Err(Error::InvalidArguments),
        }
    }
}

impl PipelineRunContextQuery {
    /// Resolve the exact stored snapshot only after the authenticated copy gate.
    pub fn resolve_receipt_diff(
        &self,
        context: &PipelineRunContext,
        digest_port: &dyn crate::PipelineDefinitionDigestPort,
    ) -> Result<PipelineReceiptDiffRead> {
        self.validate()?;
        if self.view != PipelineRunContextView::ReceiptDiff {
            return Err(Error::InvalidArguments);
        }
        if self.run_id != context.run.id {
            return Err(Error::NotFound);
        }
        if self
            .definition_digest
            .as_ref()
            .is_some_and(|pin| pin != &context.run.definition_digest)
        {
            return Err(Error::refused_at(
                RefusalCode::MethodVersionUnavailable,
                "PIPELINE-RECEIPT-DIFF-DEFINITION-PIN",
                "arguments.params.definition_digest",
                "stored definition digest",
                "definition pin mismatch",
                "restart_receipt_diff_read",
                "definition_digest",
            ));
        }
        let phase = context
            .definition
            .phases
            .iter()
            .find(|phase| Some(phase.id.as_str()) == self.phase_id.as_deref())
            .ok_or(Error::NotFound)?;
        let receipt_kind = self.receipt_kind.ok_or(Error::InvalidArguments)?;
        let submitted = self
            .submitted_receipts
            .as_deref()
            .ok_or(Error::InvalidArguments)?;
        let submitted_digest =
            crate::pipeline_receipt_multiset_digest(receipt_kind, submitted, digest_port)?;
        if self
            .submitted_digest
            .as_ref()
            .is_some_and(|pin| pin != &submitted_digest)
        {
            return Err(Error::refused_at(
                RefusalCode::InvalidOutput,
                "PIPELINE-RECEIPT-DIFF-SUBMITTED-PIN",
                "arguments.params.submitted_digest",
                "recomputed submitted receipt multiset digest",
                "submitted digest mismatch",
                "restart_receipt_diff_read",
                "submitted_digest",
            ));
        }
        let expected = match receipt_kind {
            PipelineReceiptKind::Skill => &phase.skills,
            PipelineReceiptKind::Resource => &phase.resources,
        };
        let expected = expected
            .iter()
            .map(|receipt| {
                (
                    receipt.id.as_str(),
                    receipt.version.as_str(),
                    receipt.digest.as_str(),
                )
            })
            .collect::<Vec<_>>();
        let submitted = submitted
            .iter()
            .map(|receipt| {
                (
                    receipt.instruction_id.as_str(),
                    receipt.version.as_str(),
                    receipt.digest.as_str(),
                )
            })
            .collect::<Vec<_>>();
        Ok(PipelineReceiptDiffRead {
            run_id: context.run.id,
            phase_id: phase.id.clone(),
            definition_version: context.run.definition_version.clone(),
            definition_digest: context.run.definition_digest.clone(),
            receipt_kind,
            submitted_digest,
            diff: crate::FullReceiptDiff::between(&expected, &submitted),
        })
    }
}
