use crate::{EngineeringCandidate, Error, Result, SliceCandidateNode};
use serde::Serialize;
use sha2::{Digest, Sha256};
use uuid::Uuid;

/// Exact saved content attributed to one selected Matrix choice.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MatrixPlanningEffectNode {
    pub draft_index: usize,
    pub node_id: Uuid,
    pub node_revision: i64,
    pub body: SliceCandidateNode,
}

/// Server assembled evidence; callers cannot provide its fields to the verifier.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
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
        let bytes = serde_json::to_vec(self).map_err(|_| Error::InternalInvariant)?;
        let mut hash = Sha256::new();
        hash.update(b"tect.matrix-planning-effect/1\0");
        hash.update(bytes);
        Ok(format!("{:x}", hash.finalize()))
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
}
