use super::NativeSlice;
use crate::{Error, PIPELINE_VERIFICATION_PLAN_SCHEMA, Result};

impl NativeSlice {
    pub fn validate_verification_plan_binding(&self) -> Result<()> {
        let fields = [
            self.selected_option_id.as_ref(),
            self.verification_plan_id.as_ref(),
            self.verification_plan_schema.as_ref(),
            self.verification_plan_digest.as_ref(),
            self.verification_plan_source_definition_version.as_ref(),
            self.verification_plan_source_definition_digest.as_ref(),
        ];
        if fields.iter().all(Option::is_none) {
            return Ok(());
        }
        if fields.iter().any(Option::is_none) {
            return Err(Error::InputConflict);
        }
        let plan_id = self.verification_plan_id.as_deref().unwrap();
        let digest = self.verification_plan_digest.as_deref().unwrap();
        let option_id = self.selected_option_id.as_deref().unwrap();
        if self.verification_plan_schema.as_deref() != Some(PIPELINE_VERIFICATION_PLAN_SCHEMA)
            || digest.len() != 64
            || !digest
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
            || plan_id != format!("verification-plan:{digest}")
            || option_id != format!("{}+{plan_id}", self.pipeline.as_str())
            || self
                .verification_plan_source_definition_version
                .as_deref()
                .unwrap()
                .trim()
                .is_empty()
            || self
                .verification_plan_source_definition_digest
                .as_deref()
                .unwrap()
                .trim()
                .is_empty()
        {
            return Err(Error::InputConflict);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{PipelineKind, SliceState};
    use uuid::Uuid;

    fn slice() -> NativeSlice {
        NativeSlice {
            id: Uuid::new_v4(),
            scope_id: Uuid::new_v4(),
            revision: 1,
            candidate_id: Uuid::new_v4(),
            candidate_revision: 1,
            opening_snapshot_id: Uuid::new_v4(),
            title: "slice".into(),
            outcome: "outcome".into(),
            pipeline: PipelineKind::LightweightTddDevelopment,
            selected_option_id: None,
            verification_plan_id: None,
            verification_plan_schema: None,
            verification_plan_digest: None,
            verification_plan_source_definition_version: None,
            verification_plan_source_definition_digest: None,
            state: SliceState::Open,
            pipeline_status: "not_started".into(),
            pipeline_run_id: None,
            knowledge_change_id: None,
            knowledge_run_id: None,
            knowledge_status: None,
            source_checkpoint: None,
            execution_claimed: false,
        }
    }

    #[test]
    fn plan_binding_is_all_or_none_and_exact_pair() {
        let mut slice = slice();
        assert!(slice.validate_verification_plan_binding().is_ok());
        slice.selected_option_id = Some("invented".into());
        assert_eq!(
            slice.validate_verification_plan_binding(),
            Err(Error::InputConflict)
        );
        let digest = "a".repeat(64);
        let id = format!("verification-plan:{digest}");
        slice.selected_option_id = Some(format!("{}+{id}", slice.pipeline.as_str()));
        slice.verification_plan_id = Some(id);
        slice.verification_plan_schema = Some(PIPELINE_VERIFICATION_PLAN_SCHEMA.into());
        slice.verification_plan_digest = Some(digest);
        slice.verification_plan_source_definition_version = Some("1".into());
        slice.verification_plan_source_definition_digest = Some("definition".into());
        assert!(slice.validate_verification_plan_binding().is_ok());
        slice.selected_option_id = Some("other".into());
        assert_eq!(
            slice.validate_verification_plan_binding(),
            Err(Error::InputConflict)
        );
    }

    fn bound_slice() -> NativeSlice {
        let mut value = slice();
        let digest = "a".repeat(64);
        let id = format!("verification-plan:{digest}");
        value.selected_option_id = Some(format!("{}+{id}", value.pipeline.as_str()));
        value.verification_plan_id = Some(id);
        value.verification_plan_schema = Some(PIPELINE_VERIFICATION_PLAN_SCHEMA.into());
        value.verification_plan_digest = Some(digest);
        value.verification_plan_source_definition_version = Some("1".into());
        value.verification_plan_source_definition_digest = Some("definition".into());
        value
    }

    #[test]
    fn plan_binding_checks_all_64_presence_masks() {
        for mask in 0_u8..64 {
            let mut value = bound_slice();
            for (bit, field) in [
                &mut value.selected_option_id,
                &mut value.verification_plan_id,
                &mut value.verification_plan_schema,
                &mut value.verification_plan_digest,
                &mut value.verification_plan_source_definition_version,
                &mut value.verification_plan_source_definition_digest,
            ]
            .into_iter()
            .enumerate()
            {
                if mask & (1 << bit) == 0 {
                    *field = None;
                }
            }
            let expected = if mask == 0 || mask == 63 {
                Ok(())
            } else {
                Err(Error::InputConflict)
            };
            assert_eq!(
                value.validate_verification_plan_binding(),
                expected,
                "mask={mask:06b}"
            );
        }
    }

    #[test]
    fn plan_binding_rejects_each_malformed_bound_field() {
        let invalid = [
            (2, "wrong-schema".to_string()),
            (3, "a".repeat(63)),
            (3, "a".repeat(65)),
            (3, "A".repeat(64)),
            (3, "g".repeat(64)),
            (1, format!("verification-plan:{}", "b".repeat(64))),
            (
                0,
                format!(
                    "slice.full-design-to-execution+verification-plan:{}",
                    "a".repeat(64)
                ),
            ),
            (4, " \t\n".into()),
            (5, " \t\n".into()),
        ];
        for (index, bad) in invalid {
            let mut value = bound_slice();
            // Keep the pair internally consistent to isolate digest-shape rejection.
            if index == 3 {
                let id = format!("verification-plan:{bad}");
                value.selected_option_id = Some(format!("{}+{id}", value.pipeline.as_str()));
                value.verification_plan_id = Some(id);
            }
            let fields = [
                &mut value.selected_option_id,
                &mut value.verification_plan_id,
                &mut value.verification_plan_schema,
                &mut value.verification_plan_digest,
                &mut value.verification_plan_source_definition_version,
                &mut value.verification_plan_source_definition_digest,
            ];
            *fields.into_iter().nth(index).unwrap() = Some(bad);
            assert_eq!(
                value.validate_verification_plan_binding(),
                Err(Error::InputConflict),
                "field={index}"
            );
        }
    }
}
