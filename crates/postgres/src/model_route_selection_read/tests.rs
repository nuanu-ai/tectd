use super::*;
use tect_application::{
    MatrixPlanningEffectSnapshot, MatrixPlanningMappedNode, MatrixPlanningSelectionLink,
};
use tect_domain::{
    EngineeringCandidate, MatrixPlanningContextProvenance, MatrixPlanningSelection,
    ModelRouteCallerFacts, PipelineKind,
};

// Projection is invoked only after the shared attested-ready source reader succeeds.
fn project(
    snapshot: MatrixPlanningEffectSnapshot,
    disposition: Uuid,
    work: Uuid,
    revision: i64,
    _workspace: Uuid,
    request: &Value,
) -> Result<ModelRouteWorkContext> {
    let ready_work = snapshot.saved_nodes[0].clone();
    super::exact_work_context(snapshot, disposition, work, revision, &ready_work, request)
}

fn request(snapshot: &MatrixPlanningEffectSnapshot) -> Value {
    let SliceCandidateNode::Work {
        model_route_facts, ..
    } = &snapshot.saved_nodes[0]
    else {
        panic!("expected Work")
    };
    let mut node = serde_json::json!({"kind":"work"});
    if let Some(facts) = model_route_facts {
        node["model_route_facts"] = serde_json::to_value(facts).unwrap();
    }
    serde_json::json!({"draft":{"nodes":[node]}})
}

fn snapshot() -> (Uuid, MatrixPlanningEffectSnapshot, Uuid) {
    let workspace = Uuid::new_v4();
    let node_id = Uuid::new_v4();
    let selected_choice = EngineeringCandidate {
        candidate_id: "choice-a".into(),
        title: "Choice".into(),
        approach: "Approach".into(),
        assumption_fact_ids: vec![],
    };
    let link = MatrixPlanningSelectionLink {
        selection: MatrixPlanningSelection {
            task_id: Uuid::new_v4(),
            task_revision: 1,
            disposition_id: Uuid::new_v4(),
            selected_choice_id: selected_choice.candidate_id.clone(),
            expected_input_digest: "a".repeat(64),
            expected_choice_set_digest: "b".repeat(64),
            expected_verification_digest: "c".repeat(64),
            mapped_draft_node_indices: vec![0],
        },
        evaluation_digest: "d".repeat(64),
        context_provenance: None,
        catalogue_version: "EM@1".into(),
        caller_principal_id: Uuid::new_v4(),
        caller_session_id: Uuid::new_v4(),
        scope_id: Uuid::new_v4(),
        candidate_set_id: Uuid::new_v4(),
        caller_request_id: Uuid::new_v4(),
        result_revision: 2,
        mapped_nodes: vec![MatrixPlanningMappedNode {
            draft_index: 0,
            node_id,
            node_revision: 1,
        }],
    };
    let snapshot = MatrixPlanningEffectSnapshot {
        link,
        receipt_present: true,
        selected_choice,
        matrix_owner_principal_id: Uuid::new_v4(),
        saved_nodes: vec![SliceCandidateNode::Work {
            model_route_facts: None,
            id: node_id,
            revision: 1,
            title: "Implement".into(),
            outcome: "Ship".into(),
            includes: vec!["role=agent;tool=code;budget=999".into()],
            excludes: vec![],
            dependencies: vec![],
            proof: vec!["proof".into()],
            pipeline: PipelineKind::LightweightTddDevelopment,
            pipeline_reason: "Small work".into(),
            why_lightweight_insufficient: None,
            why_further_vertical_split_not_viable: None,
            source_result_ids: vec![],
            source_checkpoint: None,
        }],
        current_result_revision: 2,
        is_current: true,
    };
    (workspace, snapshot, node_id)
}

#[test]
fn exact_saved_work_mapping_keeps_prose_facts_unknown() {
    let (workspace, snapshot, node_id) = snapshot();
    let disposition_id = snapshot.link.selection.disposition_id;
    let request = request(&snapshot);
    let context = project(
        snapshot.clone(),
        disposition_id,
        node_id,
        1,
        workspace,
        &request,
    )
    .unwrap();
    assert_eq!(
        context.selection_link.candidate_set_id,
        snapshot.link.candidate_set_id
    );
    assert_eq!(
        context.selection_link.caller_request_id,
        snapshot.link.caller_request_id
    );
    assert_eq!(context.selection_link.mapped_work_node_id, node_id);
    assert!(context.context_authority.is_none());
    assert!(context.has_unknown_facts());
    assert!(matches!(context.role, ModelRouteFact::Unknown));
    assert!(matches!(context.tool, ModelRouteFact::Unknown));
    assert!(matches!(context.data_class, ModelRouteFact::Unknown));
    assert!(matches!(context.host_capabilities, ModelRouteFact::Unknown));
    assert!(matches!(
        context.remaining_budget_units,
        ModelRouteFact::Unknown
    ));
    assert!(matches!(
        context.available_latency_ms,
        ModelRouteFact::Unknown
    ));
}

#[test]
fn v2_selection_projects_saved_authority_tuple() {
    let (workspace, mut snapshot, node_id) = snapshot();
    let frozen_snapshot_id = Uuid::new_v4();
    snapshot.link.context_provenance = Some(MatrixPlanningContextProvenance {
        frozen_snapshot_id,
        authority_schema: tect_domain::MATRIX_REQUIREMENTS_SCHEMA.into(),
        requirements_semantic_digest: "e".repeat(64),
    });
    let request = request(&snapshot);
    let context = project(
        snapshot.clone(),
        snapshot.link.selection.disposition_id,
        node_id,
        1,
        workspace,
        &request,
    )
    .unwrap();
    assert_eq!(
        context.context_authority,
        Some(ModelRouteContextAuthority {
            frozen_snapshot_id,
            authority_schema: tect_domain::MATRIX_REQUIREMENTS_SCHEMA.into(),
            requirements_semantic_digest: "e".repeat(64),
            operating_verification_digest: snapshot.link.selection.expected_verification_digest,
        })
    );
}

#[test]
fn stale_wrong_or_decision_mapping_fails_closed() {
    let (workspace, snapshot, node_id) = snapshot();
    let disposition_id = snapshot.link.selection.disposition_id;
    let request = request(&snapshot);
    assert!(
        project(
            snapshot.clone(),
            disposition_id,
            Uuid::new_v4(),
            1,
            workspace,
            &request
        )
        .is_err()
    );
    assert!(
        project(
            snapshot.clone(),
            disposition_id,
            node_id,
            2,
            workspace,
            &request
        )
        .is_err()
    );
    assert!(
        project(
            snapshot.clone(),
            Uuid::new_v4(),
            node_id,
            1,
            workspace,
            &request
        )
        .is_err()
    );
    let mut stale = snapshot.clone();
    stale.is_current = false;
    // Ready review advances the head beyond the saved draft; the shared reader,
    // rather than snapshot.material(), proves the authoritative current body.
    assert!(
        project(
            stale.clone(),
            disposition_id,
            node_id,
            1,
            workspace,
            &request
        )
        .is_ok()
    );
    let mut changed_body = stale.saved_nodes[0].clone();
    if let SliceCandidateNode::Work { title, .. } = &mut changed_body {
        title.push_str(" changed");
    }
    assert!(
        super::exact_work_context(stale, disposition_id, node_id, 1, &changed_body, &request)
            .is_err()
    );
    let mut decision = snapshot;
    decision.saved_nodes[0] = SliceCandidateNode::Decision {
        id: node_id,
        revision: 1,
        title: "Decision".into(),
        question: "Why?".into(),
        resolution_criteria: vec!["Proof".into()],
        dependencies: vec![],
        source_result_ids: vec![],
    };
    assert!(project(decision, disposition_id, node_id, 1, workspace, &request).is_err());
}

#[test]
fn exact_typed_caller_facts_are_field_specific_assertions() {
    let (workspace, mut snapshot, node_id) = snapshot();
    let disposition_id = snapshot.link.selection.disposition_id;
    let old_effect_digest = snapshot.effect_digest(workspace).unwrap();
    let SliceCandidateNode::Work {
        model_route_facts, ..
    } = &mut snapshot.saved_nodes[0]
    else {
        unreachable!()
    };
    *model_route_facts = Some(Box::new(ModelRouteCallerFacts {
        role: Some("agent".into()),
        tool: Some("code".into()),
        data_class: Some("internal".into()),
        remaining_budget_units: Some(10),
        available_latency_ms: Some(50),
    }));
    assert_ne!(
        old_effect_digest,
        snapshot.effect_digest(workspace).unwrap()
    );
    let snapshot_request = request(&snapshot);
    let context = project(
        snapshot.clone(),
        disposition_id,
        node_id,
        1,
        workspace,
        &snapshot_request,
    )
    .unwrap();
    for (fact, field, value) in [
        (&context.role, "role", "agent"),
        (&context.tool, "tool", "code"),
        (&context.data_class, "data_class", "internal"),
    ] {
        assert!(matches!(fact, ModelRouteFact::Known {
                value: actual,
                provenance: ModelRouteFactProvenance::Caller {
                    source_ref, work_node_id, work_node_revision,
                },
            } if actual == value
                && source_ref == &format!("native_planning_receipt/{}/{}#/draft/nodes/0/model_route_facts/{field}", snapshot.link.candidate_set_id, snapshot.link.caller_request_id)
                && *work_node_id == node_id && *work_node_revision == 1));
    }
    assert!(
        matches!(context.remaining_budget_units, ModelRouteFact::Known { value: 10, provenance: ModelRouteFactProvenance::Caller { ref source_ref, .. } } if source_ref.ends_with("/remaining_budget_units"))
    );
    assert!(
        matches!(context.available_latency_ms, ModelRouteFact::Known { value: 50, provenance: ModelRouteFactProvenance::Caller { ref source_ref, .. } } if source_ref.ends_with("/available_latency_ms"))
    );
    assert!(matches!(context.host_capabilities, ModelRouteFact::Unknown));
    assert!(context.has_unknown_facts());
    let mut partial = snapshot.clone();
    if let SliceCandidateNode::Work {
        model_route_facts: Some(facts),
        ..
    } = &mut partial.saved_nodes[0]
    {
        facts.tool = None;
    }
    let partial_request = request(&partial);
    let partial_context = project(
        partial,
        disposition_id,
        node_id,
        1,
        workspace,
        &partial_request,
    )
    .unwrap();
    assert!(matches!(partial_context.tool, ModelRouteFact::Unknown));
    assert!(matches!(partial_context.role, ModelRouteFact::Known { .. }));
    let mut conflict = snapshot_request;
    conflict["draft"]["nodes"][0]["model_route_facts"]["role"] = serde_json::json!("owner");
    assert!(project(snapshot, disposition_id, node_id, 1, workspace, &conflict).is_err());
}

#[test]
fn ready_body_and_mapping_shape_must_match_exactly() {
    let (_, snapshot, work_id) = snapshot();
    let disposition = snapshot.link.selection.disposition_id;
    let request = request(&snapshot);
    let ready_work = snapshot.saved_nodes[0].clone();
    let mut missing_mapping = snapshot.clone();
    missing_mapping.link.mapped_nodes.clear();
    assert!(
        super::exact_work_context(
            missing_mapping,
            disposition,
            work_id,
            1,
            &ready_work,
            &request
        )
        .is_err()
    );
    let mut duplicate_mapping = snapshot.clone();
    duplicate_mapping
        .link
        .mapped_nodes
        .push(snapshot.link.mapped_nodes[0].clone());
    duplicate_mapping.saved_nodes.push(ready_work.clone());
    assert!(
        super::exact_work_context(
            duplicate_mapping,
            disposition,
            work_id,
            1,
            &ready_work,
            &request
        )
        .is_err()
    );
    let mut changed = ready_work.clone();
    if let SliceCandidateNode::Work { title, .. } = &mut changed {
        title.push_str(" changed");
    }
    assert!(
        super::exact_work_context(snapshot, disposition, work_id, 1, &changed, &request).is_err()
    );
}
