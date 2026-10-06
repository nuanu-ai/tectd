use super::*;

impl BeginPipelineRun {
    pub fn validate(&self, definition: &PipelineDefinitionSnapshot) -> Result<()> {
        let inquiry_valid = match definition.kind {
            PipelineKind::Research => self
                .inquiry
                .as_ref()
                .is_some_and(|inquiry| inquiry.require_research().is_ok()),
            PipelineKind::DeepBrainstorming => self
                .inquiry
                .as_ref()
                .is_some_and(|inquiry| inquiry.validate().is_ok() && inquiry.is_decision()),
            _ => self.inquiry.is_none(),
        };
        let checkpoint_valid = match definition.kind {
            PipelineKind::Research => self
                .source_checkpoint
                .as_ref()
                .is_none_or(|checkpoint| checkpoint.validate().is_ok()),
            _ => self.source_checkpoint.is_none(),
        };
        if self.request_id.is_nil() {
            return Err(schema_refusal(
                "WP6-BEGIN-REQUEST-ID",
                "arguments.params.request_id",
                "non-nil request UUID",
                "nil UUID",
            ));
        }
        if self.scope_id.is_nil() {
            return Err(schema_refusal(
                "WP6-BEGIN-SCOPE-ID",
                "arguments.params.scope_id",
                "non-nil scope UUID",
                "nil UUID",
            ));
        }
        if self.slice_id.is_nil() {
            return Err(schema_refusal(
                "WP6-BEGIN-SLICE-ID",
                "arguments.params.slice_id",
                "non-nil slice UUID",
                "nil UUID",
            ));
        }
        if self.slice_revision < 1 {
            return Err(schema_refusal(
                "WP6-BEGIN-SLICE-REVISION",
                "arguments.params.slice_revision",
                "positive slice revision",
                "nonpositive",
            ));
        }
        if self.qualification_reason.trim().is_empty() {
            return Err(schema_refusal(
                "WP6-BEGIN-QUALIFICATION",
                "arguments.params.qualification_reason",
                "nonblank qualification reason",
                "blank",
            ));
        }
        if let Some(version) = self.definition_version.as_deref() {
            if version.trim().is_empty() {
                return Err(schema_refusal(
                    "WP6-BEGIN-DEFINITION-VERSION-BLANK",
                    "arguments.params.definition_version",
                    "nonblank pinned definition version",
                    "blank",
                ));
            }
            if version.len() > 128 {
                return Err(schema_refusal(
                    "WP6-BEGIN-DEFINITION-VERSION-LIMIT",
                    "arguments.params.definition_version",
                    "at most 128 UTF-8 bytes",
                    format!("bytes={}", version.len()),
                ));
            }
            if version != definition.version {
                return Err(schema_refusal(
                    "WP6-BEGIN-DEFINITION-VERSION-PIN",
                    "arguments.params.definition_version",
                    "version of selected pinned definition",
                    "mismatched",
                ));
            }
        }
        if !definition
            .allowed_modes
            .contains(&self.delivery_mode.unwrap_or(definition.default_mode))
        {
            return Err(schema_refusal(
                "WP6-BEGIN-DELIVERY-MODE",
                "arguments.params.delivery_mode",
                "mode allowed by selected definition",
                format!(
                    "{:?}",
                    self.delivery_mode.unwrap_or(definition.default_mode)
                ),
            ));
        }
        if !inquiry_valid {
            return Err(schema_refusal(
                "WP6-BEGIN-INQUIRY",
                "arguments.params.inquiry",
                "inquiry satisfying selected kind's contract; omitted for other kinds",
                if self.inquiry.is_none() {
                    "missing"
                } else {
                    "present but not valid for selected kind"
                },
            ));
        }
        if !checkpoint_valid {
            return Err(schema_refusal(
                "WP6-BEGIN-SOURCE-CHECKPOINT",
                "arguments.params.source_checkpoint",
                "valid optional checkpoint for research; omitted for other kinds",
                "present but invalid for selected kind",
            ));
        }
        Ok(())
    }
}
