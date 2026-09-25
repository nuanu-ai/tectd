use serde::{Deserialize, Serialize};

/// Caller-authored route constraints attached to one saved Work revision.
/// Each present field is an assertion, not independently verified evidence.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelRouteCallerFacts {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_class: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remaining_budget_units: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub available_latency_ms: Option<u64>,
}

impl ModelRouteCallerFacts {
    pub fn validate(&self) -> crate::Result<()> {
        if self.role.is_none()
            && self.tool.is_none()
            && self.data_class.is_none()
            && self.remaining_budget_units.is_none()
            && self.available_latency_ms.is_none()
        {
            return Err(crate::Error::InvalidArguments);
        }
        for value in [&self.role, &self.tool, &self.data_class]
            .into_iter()
            .flatten()
        {
            if value.is_empty()
                || value.len() > 128
                || !value.bytes().all(|byte| {
                    byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b'/' | b':')
                })
            {
                return Err(crate::Error::InvalidArguments);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{PipelineKind, SliceCandidateDraftNode, SliceCandidateNode};
    use uuid::Uuid;

    #[test]
    fn old_work_payload_omits_facts_and_typed_fields_round_trip() {
        let legacy = SliceCandidateNode::Work {
            id: Uuid::new_v4(),
            revision: 1,
            model_route_facts: None,
            title: "Work".into(),
            outcome: "Outcome".into(),
            includes: vec![],
            excludes: vec![],
            dependencies: vec![],
            proof: vec!["Proof".into()],
            pipeline: PipelineKind::LightweightTddDevelopment,
            pipeline_reason: "Small".into(),
            why_lightweight_insufficient: None,
            why_further_vertical_split_not_viable: None,
            source_result_ids: vec![],
            source_checkpoint: None,
        };
        let encoded = serde_json::to_value(&legacy).unwrap();
        assert!(encoded.get("model_route_facts").is_none());
        assert_eq!(
            serde_json::from_value::<SliceCandidateNode>(encoded.clone()).unwrap(),
            legacy
        );

        let mut typed = legacy;
        if let SliceCandidateNode::Work {
            model_route_facts, ..
        } = &mut typed
        {
            *model_route_facts = Some(Box::new(ModelRouteCallerFacts {
                role: Some("agent".into()),
                tool: Some("code".into()),
                data_class: Some("internal".into()),
                remaining_budget_units: Some(10),
                available_latency_ms: Some(50),
            }));
        }
        let typed_json = serde_json::to_value(&typed).unwrap();
        let mut expected_json = encoded;
        expected_json["model_route_facts"] = serde_json::json!({
            "role": "agent", "tool": "code", "data_class": "internal",
            "remaining_budget_units": 10, "available_latency_ms": 50
        });
        assert_eq!(typed_json, expected_json);
        assert_eq!(
            serde_json::from_value::<SliceCandidateNode>(typed_json).unwrap(),
            typed
        );

        let mut draft: SliceCandidateDraftNode = serde_json::from_value(serde_json::json!({
            "kind": "work", "identity": {"local": "work"},
            "title": "Work", "outcome": "Outcome", "proof": ["Proof"],
            "pipeline": PipelineKind::LightweightTddDevelopment, "pipeline_reason": "Small"
        }))
        .unwrap();
        let absent_draft_json = serde_json::to_value(&draft).unwrap();
        assert!(absent_draft_json.get("model_route_facts").is_none());
        if let SliceCandidateDraftNode::Work {
            model_route_facts, ..
        } = &mut draft
        {
            *model_route_facts = Some(Box::new(ModelRouteCallerFacts {
                role: Some("agent".into()),
                ..Default::default()
            }));
        }
        let mut expected_draft_json = absent_draft_json;
        expected_draft_json["model_route_facts"] = serde_json::json!({"role": "agent"});
        assert_eq!(serde_json::to_value(&draft).unwrap(), expected_draft_json);
        assert_eq!(
            serde_json::from_value::<SliceCandidateDraftNode>(expected_draft_json).unwrap(),
            draft
        );
    }

    #[test]
    fn caller_facts_reject_empty_or_unusable_values() {
        assert!(ModelRouteCallerFacts::default().validate().is_err());
        assert!(
            ModelRouteCallerFacts {
                role: Some(" agent".into()),
                ..Default::default()
            }
            .validate()
            .is_err()
        );
        assert!(
            ModelRouteCallerFacts {
                role: Some("agent".into()),
                ..Default::default()
            }
            .validate()
            .is_ok()
        );
        let unknown = serde_json::json!({"role":"agent", "host_capabilities":["gpu"]});
        assert!(serde_json::from_value::<ModelRouteCallerFacts>(unknown).is_err());
    }
}
