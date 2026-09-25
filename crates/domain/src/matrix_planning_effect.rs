use crate::{EngineeringCandidate, Error, PipelineKind, Result, SliceCandidateNode};
use sha2::{Digest, Sha256};
use uuid::Uuid;

/// Exact saved content attributed to one selected Matrix choice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatrixPlanningEffectNode {
    pub draft_index: usize,
    pub node_id: Uuid,
    pub node_revision: i64,
    pub body: SliceCandidateNode,
}

/// Server assembled evidence; callers cannot provide its fields to the verifier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatrixPlanningEffectMaterial {
    pub workspace_id: Uuid,
    pub candidate_set_id: Uuid,
    pub caller_request_id: Uuid,
    pub scope_id: Uuid,
    pub result_revision: i64,
    pub task_id: Uuid,
    pub task_revision: i64,
    pub disposition_id: Uuid,
    pub input_digest: String,
    pub choice_set_digest: String,
    pub verification_digest: String,
    pub evaluation_digest: String,
    pub catalogue_version: String,
    pub caller_principal_id: Uuid,
    pub caller_session_id: Uuid,
    pub matrix_owner_principal_id: Uuid,
    pub selected_choice: EngineeringCandidate,
    pub nodes: Vec<MatrixPlanningEffectNode>,
}

impl MatrixPlanningEffectMaterial {
    pub fn canonical_digest(&self) -> Result<String> {
        if self.workspace_id.is_nil()
            || self.candidate_set_id.is_nil()
            || self.caller_request_id.is_nil()
            || self.scope_id.is_nil()
            || self.task_id.is_nil()
            || self.disposition_id.is_nil()
            || self.caller_principal_id.is_nil()
            || self.caller_session_id.is_nil()
            || self.matrix_owner_principal_id.is_nil()
            || self.result_revision < 1
            || self.task_revision < 1
            || self.selected_choice.candidate_id.trim().is_empty()
            || self.nodes.is_empty()
            || self.nodes.len() > 100
        {
            return Err(Error::StaleContext);
        }
        for (position, node) in self.nodes.iter().enumerate() {
            if node.node_id.is_nil()
                || node.node_revision < 1
                || node.node_id != node.body.id()
                || node.node_revision != node.body.revision()
                || (position > 0 && self.nodes[position - 1].draft_index >= node.draft_index)
            {
                return Err(Error::StaleContext);
            }
        }
        let mut hash = CanonicalHash(Sha256::new());
        hash.string("tect.matrix-planning-effect/1");
        hash.uuid(self.workspace_id);
        hash.uuid(self.candidate_set_id);
        hash.uuid(self.caller_request_id);
        hash.uuid(self.scope_id);
        hash.i64(self.result_revision);
        hash.uuid(self.task_id);
        hash.i64(self.task_revision);
        hash.uuid(self.disposition_id);
        hash.string(&self.input_digest);
        hash.string(&self.choice_set_digest);
        hash.string(&self.verification_digest);
        hash.string(&self.evaluation_digest);
        hash.string(&self.catalogue_version);
        hash.uuid(self.caller_principal_id);
        hash.uuid(self.caller_session_id);
        hash.uuid(self.matrix_owner_principal_id);
        hash.string(&self.selected_choice.candidate_id);
        hash.string(&self.selected_choice.title);
        hash.string(&self.selected_choice.approach);
        hash.strings(&self.selected_choice.assumption_fact_ids);
        hash.len(self.nodes.len());
        for node in &self.nodes {
            hash.len(node.draft_index);
            hash.uuid(node.node_id);
            hash.i64(node.node_revision);
            hash.node(&node.body);
        }
        Ok(format!("{:x}", hash.0.finalize()))
    }
}

/// Each variable field is length-prefixed; options and variants have explicit
/// tags. This encoding is independent of Serde and Rust debug formatting.
struct CanonicalHash(Sha256);

impl CanonicalHash {
    fn bytes(&mut self, value: &[u8]) {
        self.0.update((value.len() as u64).to_be_bytes());
        self.0.update(value);
    }

    fn string(&mut self, value: &str) {
        self.bytes(value.as_bytes());
    }

    fn len(&mut self, value: usize) {
        self.0.update((value as u64).to_be_bytes());
    }

    fn i64(&mut self, value: i64) {
        self.0.update(value.to_be_bytes());
    }

    fn uuid(&mut self, value: Uuid) {
        self.0.update(value.as_bytes());
    }

    fn uuids(&mut self, values: &[Uuid]) {
        self.len(values.len());
        for value in values {
            self.uuid(*value);
        }
    }

    fn strings(&mut self, values: &[String]) {
        self.len(values.len());
        for value in values {
            self.string(value);
        }
    }

    fn optional_string(&mut self, value: Option<&str>) {
        match value {
            Some(value) => {
                self.0.update([1]);
                self.string(value);
            }
            None => self.0.update([0]),
        }
    }

    fn optional_u64(&mut self, value: Option<u64>) {
        match value {
            Some(value) => {
                self.0.update([1]);
                self.0.update(value.to_be_bytes());
            }
            None => self.0.update([0]),
        }
    }

    fn node(&mut self, node: &SliceCandidateNode) {
        match node {
            SliceCandidateNode::Work {
                id,
                revision,
                model_route_facts,
                title,
                outcome,
                includes,
                excludes,
                dependencies,
                proof,
                pipeline,
                pipeline_reason,
                why_lightweight_insufficient,
                why_further_vertical_split_not_viable,
                source_result_ids,
                source_checkpoint,
            } => {
                self.0.update([1]);
                self.uuid(*id);
                self.i64(*revision);
                self.string(title);
                self.string(outcome);
                self.strings(includes);
                self.strings(excludes);
                self.uuids(dependencies);
                self.strings(proof);
                self.string(pipeline_name(*pipeline));
                self.string(pipeline_reason);
                self.optional_string(why_lightweight_insufficient.as_deref());
                self.optional_string(why_further_vertical_split_not_viable.as_deref());
                self.uuids(source_result_ids);
                match source_checkpoint {
                    Some(checkpoint) => {
                        self.0.update([1]);
                        self.uuid(checkpoint.checkpoint_id);
                        self.string(&checkpoint.digest);
                    }
                    None => self.0.update([0]),
                }
                if let Some(facts) = model_route_facts {
                    self.string("tect.model-route-caller-facts/1");
                    self.optional_string(facts.role.as_deref());
                    self.optional_string(facts.tool.as_deref());
                    self.optional_string(facts.data_class.as_deref());
                    self.optional_u64(facts.remaining_budget_units);
                    self.optional_u64(facts.available_latency_ms);
                }
            }
            SliceCandidateNode::Decision {
                id,
                revision,
                title,
                question,
                resolution_criteria,
                dependencies,
                source_result_ids,
            } => {
                self.0.update([2]);
                self.uuid(*id);
                self.i64(*revision);
                self.string(title);
                self.string(question);
                self.strings(resolution_criteria);
                self.uuids(dependencies);
                self.uuids(source_result_ids);
            }
        }
    }
}

fn pipeline_name(kind: PipelineKind) -> &'static str {
    match kind {
        PipelineKind::LightweightTddDevelopment => "lightweight_tdd_development",
        PipelineKind::FullDesignToExecution => "full_design_to_execution",
        PipelineKind::DebugRootCause => "debug_root_cause",
        PipelineKind::OperationalPreparation => "operational_preparation",
        PipelineKind::OperationalExecution => "operational_execution",
        PipelineKind::Research => "research",
        PipelineKind::DeepBrainstorming => "deep_brainstorming",
        PipelineKind::ResearchToDurableKnowledge => "research_to_durable_knowledge",
        PipelineKind::CustomProcedureCapture => "custom_procedure_capture",
        PipelineKind::PromoteToDurableKnowledge => "promote_to_durable_knowledge",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digest_binds_selected_choice_and_saved_body() {
        let id = Uuid::new_v4();
        let node = SliceCandidateNode::Decision {
            id,
            revision: 2,
            title: "Select storage".into(),
            question: "Which store?".into(),
            resolution_criteria: vec!["Latency".into()],
            dependencies: vec![],
            source_result_ids: vec![],
        };
        let mut material = MatrixPlanningEffectMaterial {
            workspace_id: Uuid::new_v4(),
            candidate_set_id: Uuid::new_v4(),
            caller_request_id: Uuid::new_v4(),
            scope_id: Uuid::new_v4(),
            result_revision: 3,
            task_id: Uuid::new_v4(),
            task_revision: 1,
            disposition_id: Uuid::new_v4(),
            input_digest: "a".repeat(64),
            choice_set_digest: "b".repeat(64),
            verification_digest: "c".repeat(64),
            evaluation_digest: "d".repeat(64),
            catalogue_version: "v1".into(),
            caller_principal_id: Uuid::new_v4(),
            caller_session_id: Uuid::new_v4(),
            matrix_owner_principal_id: Uuid::new_v4(),
            selected_choice: EngineeringCandidate {
                candidate_id: "choice-a".into(),
                title: "Postgres".into(),
                approach: "Store atomically".into(),
                assumption_fact_ids: vec!["scale".into()],
            },
            nodes: vec![MatrixPlanningEffectNode {
                draft_index: 0,
                node_id: id,
                node_revision: 2,
                body: node,
            }],
        };
        let digest = material.canonical_digest().unwrap();
        assert_eq!(digest, material.canonical_digest().unwrap());
        material.selected_choice.approach.push_str(" and replicate");
        assert_ne!(digest, material.canonical_digest().unwrap());
        let digest = material.canonical_digest().unwrap();
        if let SliceCandidateNode::Decision { question, .. } = &mut material.nodes[0].body {
            question.push_str(" Now?");
        }
        assert_ne!(digest, material.canonical_digest().unwrap());
    }

    #[test]
    fn canonical_digest_has_stable_wire_value() {
        let id = Uuid::from_u128(8);
        let mut material = MatrixPlanningEffectMaterial {
            workspace_id: Uuid::from_u128(1),
            candidate_set_id: Uuid::from_u128(2),
            caller_request_id: Uuid::from_u128(3),
            scope_id: Uuid::from_u128(4),
            result_revision: 3,
            task_id: Uuid::from_u128(5),
            task_revision: 2,
            disposition_id: Uuid::from_u128(6),
            input_digest: "a".repeat(64),
            choice_set_digest: "b".repeat(64),
            verification_digest: "c".repeat(64),
            evaluation_digest: "d".repeat(64),
            catalogue_version: "EM@1".into(),
            caller_principal_id: Uuid::from_u128(7),
            caller_session_id: Uuid::from_u128(9),
            matrix_owner_principal_id: Uuid::from_u128(10),
            selected_choice: EngineeringCandidate {
                candidate_id: "choice-a".into(),
                title: "Choice A".into(),
                approach: "Add a saved node".into(),
                assumption_fact_ids: vec!["scale".into()],
            },
            nodes: vec![MatrixPlanningEffectNode {
                draft_index: 0,
                node_id: id,
                node_revision: 1,
                body: SliceCandidateNode::Decision {
                    id,
                    revision: 1,
                    title: "Decision".into(),
                    question: "Approve A?".into(),
                    resolution_criteria: vec!["Observed".into()],
                    dependencies: vec![],
                    source_result_ids: vec![],
                },
            }],
        };
        assert_eq!(
            material.canonical_digest().unwrap(),
            "8020e38e4f2ffbb2c7c31a28936cd27d998142ad90dadad3a23db3ddf4aac2ed"
        );
        material.nodes[0].body = SliceCandidateNode::Work {
            id,
            revision: 1,
            model_route_facts: None,
            title: "Work".into(),
            outcome: "Ship".into(),
            includes: vec![],
            excludes: vec![],
            dependencies: vec![],
            proof: vec!["Test".into()],
            pipeline: PipelineKind::LightweightTddDevelopment,
            pipeline_reason: "Small".into(),
            why_lightweight_insufficient: None,
            why_further_vertical_split_not_viable: None,
            source_result_ids: vec![],
            source_checkpoint: None,
        };
        let absent_json = serde_json::to_value(&material.nodes[0].body).unwrap();
        assert!(absent_json.get("model_route_facts").is_none());
        assert_eq!(
            material.canonical_digest().unwrap(),
            "75b1e250626e87bba771028b134dffbaf28f6f3efc18451d4b9811961304f2b9"
        );
        if let SliceCandidateNode::Work {
            model_route_facts, ..
        } = &mut material.nodes[0].body
        {
            *model_route_facts = Some(Box::new(crate::ModelRouteCallerFacts {
                role: Some("agent".into()),
                tool: Some("code".into()),
                data_class: Some("internal".into()),
                remaining_budget_units: Some(10),
                available_latency_ms: Some(50),
            }));
        }
        let mut expected_json = absent_json;
        expected_json["model_route_facts"] = serde_json::json!({
            "role": "agent", "tool": "code", "data_class": "internal",
            "remaining_budget_units": 10, "available_latency_ms": 50
        });
        assert_eq!(
            serde_json::to_value(&material.nodes[0].body).unwrap(),
            expected_json
        );
        assert_eq!(
            material.canonical_digest().unwrap(),
            "f7dc6a486e807eb7c32db697fbaddfa27666d72421d6b4752567d060954c5fe7"
        );
    }
}
