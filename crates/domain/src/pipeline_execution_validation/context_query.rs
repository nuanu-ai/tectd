use super::*;

impl PipelineRunContextQuery {
    pub fn validate(&self) -> Result<()> {
        let valid_selector = match self.view {
            PipelineRunContextView::Current => self.output_id.is_none() && self.digest.is_none(),
            PipelineRunContextView::Output => {
                self.output_id.is_some_and(|id| !id.is_nil())
                    && self
                        .digest
                        .as_ref()
                        .is_some_and(|value| !value.trim().is_empty())
            }
            PipelineRunContextView::Snapshot
            | PipelineRunContextView::PhaseContract
            | PipelineRunContextView::Details
            | PipelineRunContextView::DeliveryReceipt
            | PipelineRunContextView::ReceiptDiff => {
                self.output_id.is_none() && self.digest.is_none()
            }
        };
        if self.run_id.is_nil() {
            return Err(Error::refused_at(
                RefusalCode::InputSchemaInvalid,
                "WP6-COMPLETE-REQUEST-QUERY-01",
                "arguments.params.run_id",
                "non-nil run UUID",
                "nil UUID",
                "supply_run_id",
                "run_id",
            ));
        }
        if !valid_selector {
            return Err(Error::refused_at(
                RefusalCode::InputSchemaInvalid,
                "WP6-COMPLETE-REQUEST-QUERY-02",
                "arguments.params.view",
                "current and delivery_receipt omit output_id/digest; output requires non-nil output_id and non-empty digest",
                format!(
                    "view={:?}; output_present={}; digest_present={}",
                    self.view,
                    self.output_id.is_some(),
                    self.digest.is_some()
                ),
                "align_context_query_selector",
                "valid_context_selector",
            ));
        }
        if self.view == PipelineRunContextView::ReceiptDiff {
            self.validate_receipt_diff_selector()?;
            return crate::validate_pipeline_fragment(
                self.offset_bytes,
                self.limit_bytes,
                self.representation_digest.as_deref(),
            );
        }
        for (field, present) in [
            ("receipt_kind", self.receipt_kind.is_some()),
            ("submitted_receipts", self.submitted_receipts.is_some()),
            ("submitted_digest", self.submitted_digest.is_some()),
        ] {
            if present {
                return Err(receipt_query_refusal(
                    field,
                    "receipt-only field requires receipt_diff view",
                ));
            }
        }
        if !matches!(
            self.view,
            PipelineRunContextView::Output
                | PipelineRunContextView::Snapshot
                | PipelineRunContextView::PhaseContract
                | PipelineRunContextView::Details
        ) && (self.offset_bytes.is_some()
            || self.limit_bytes.is_some()
            || self.representation_digest.is_some())
        {
            return Err(Error::refused_at(
                RefusalCode::InputSchemaInvalid,
                "PIPELINE-JSON-FRAGMENT-VIEW",
                "arguments.params.view",
                "pagination fields only with output, snapshot, phase_contract, or details view",
                format!("{:?}", self.view),
                "select_output_view",
                "view",
            ));
        }
        let static_view = matches!(
            self.view,
            PipelineRunContextView::Snapshot | PipelineRunContextView::PhaseContract
        );
        let details = self.view == PipelineRunContextView::Details;
        if (static_view
            && self
                .definition_digest
                .as_ref()
                .is_none_or(|pin| pin.trim().is_empty()))
            || (!static_view && self.definition_digest.is_some())
            || (self.view == PipelineRunContextView::PhaseContract
                && self.phase_id.as_ref().is_none_or(|id| id.trim().is_empty()))
            || (self.view != PipelineRunContextView::PhaseContract && self.phase_id.is_some())
            || (details && self.run_revision.is_none_or(|revision| revision < 1))
            || (!details && (self.run_revision.is_some() || self.section.is_some()))
        {
            return Err(Error::refused_at(
                RefusalCode::InputSchemaInvalid,
                "PIPELINE-CONTEXT-READ-SELECTOR",
                "arguments.params.view",
                "snapshot requires definition_digest; phase_contract also phase_id; details requires positive run_revision and optional section; omit unrelated pins",
                "invalid read selector",
                "align_context_query_selector",
                "valid_context_selector",
            ));
        }
        crate::validate_pipeline_fragment(
            self.offset_bytes,
            self.limit_bytes,
            self.representation_digest.as_deref(),
        )?;
        Ok(())
    }
}

impl PipelineRunContextQuery {
    fn validate_receipt_diff_selector(&self) -> Result<()> {
        if self.phase_id.as_ref().is_none_or(|id| id.trim().is_empty()) {
            return Err(receipt_query_refusal("phase_id", "exact phase id required"));
        }
        if self.receipt_kind.is_none() {
            return Err(receipt_query_refusal(
                "receipt_kind",
                "skill or resource required",
            ));
        }
        if self.submitted_receipts.is_none() {
            return Err(receipt_query_refusal(
                "submitted_receipts",
                "receipt array required; empty array is valid",
            ));
        }
        if self.run_revision.is_some() || self.section.is_some() {
            return Err(receipt_query_refusal(
                "view",
                "receipt_diff omits run_revision and section",
            ));
        }
        if self
            .definition_digest
            .as_ref()
            .is_some_and(|pin| pin.trim().is_empty())
        {
            return Err(receipt_query_refusal(
                "definition_digest",
                "nonempty stored definition digest",
            ));
        }
        if self
            .submitted_digest
            .as_ref()
            .is_some_and(|pin| pin.len() != 64 || !pin.bytes().all(|byte| byte.is_ascii_hexdigit()))
        {
            return Err(receipt_query_refusal(
                "submitted_digest",
                "SHA256 hexadecimal digest",
            ));
        }
        if self.offset_bytes.unwrap_or(0) > 0 || self.representation_digest.is_some() {
            for (field, present) in [
                ("definition_digest", self.definition_digest.is_some()),
                ("submitted_digest", self.submitted_digest.is_some()),
                (
                    "representation_digest",
                    self.representation_digest.is_some(),
                ),
            ] {
                if !present {
                    return Err(receipt_query_refusal(
                        field,
                        "continuation requires definition, submitted, and representation digest pins",
                    ));
                }
            }
        }
        Ok(())
    }
}
fn receipt_query_refusal(field: &str, expected: &str) -> Error {
    Error::Refused(Box::new(
        Refusal::new(RefusalCode::InputSchemaInvalid)
            .with_rule("PIPELINE-RECEIPT-DIFF-SELECTOR")
            .with_path(format!("arguments.params.{field}"))
            .with_expected(expected)
            .with_actual("missing or incompatible receipt selector")
            .with_next_action("align_receipt_diff_selector")
            .with_required("receipt_diff_selector"),
    ))
}
