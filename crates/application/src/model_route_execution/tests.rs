use super::*;
use crate::{MatrixPlanningMappedNode, MatrixTaskRequirementsBinding, MatrixTaskRevision};
use sha2::Digest;
use tect_domain::{
    EngineeringMatrixInput, MatrixFact, MatrixPlanningContextProvenance, MatrixPlanningSelection,
    ModelRoute, ModelRouteContextAuthority, ModelRouteDecisionInput, ModelRouteFactProvenance,
    ModelRouteHostCapabilities, ModelRouteRanking, ModelRouteRecord, ModelRouteSelectionLink,
    ModelRouteWorkContext, OperatingEnvelope, OperationalFacts,
};

struct Fixture {
    request: PrepareModelRouteHostSelection,
    prepared: PreparedModelRouteRecommendation,
    decision: CapturedModelRouteDecision,
    disposition: CapturedModelRouteDisposition,
    source: MatrixTaskSource,
    link: MatrixPlanningSelectionLink,
}

fn id(value: u128) -> Uuid {
    Uuid::from_u128(value)
}
fn observed<T>(value: T) -> ModelRouteFact<T> {
    ModelRouteFact::Known {
        value,
        provenance: ModelRouteFactProvenance::OperatingEvidence {
            source_ref: "synthetic-test-observation".into(),
            content_digest: "e".repeat(64),
            observed_at_epoch_ms: 1,
            expires_at_epoch_ms: i64::MAX,
            work_node_id: id(8),
            work_node_revision: 2,
        },
    }
}
fn fixture() -> Fixture {
    let selection = MatrixPlanningSelection {
        task_id: id(4),
        task_revision: 3,
        disposition_id: id(5),
        selected_choice_id: "choice-a".into(),
        expected_input_digest: "a".repeat(64),
        expected_choice_set_digest: "b".repeat(64),
        expected_verification_digest: "c".repeat(64),
        mapped_draft_node_indices: vec![0],
    };
    let work = ModelRouteWorkContext {
        approved_matrix_selection: selection.clone(),
        selection_link: ModelRouteSelectionLink {
            candidate_set_id: id(6),
            caller_request_id: id(7),
            mapped_draft_node_index: 0,
            mapped_work_node_id: id(8),
            mapped_work_node_revision: 2,
        },
        context_authority: Some(ModelRouteContextAuthority {
            frozen_snapshot_id: id(9),
            authority_schema: tect_domain::MATRIX_REQUIREMENTS_SCHEMA.into(),
            requirements_semantic_digest: "d".repeat(64),
            operating_verification_digest: "c".repeat(64),
        }),
        role: observed("implementation".into()),
        tool: observed("code".into()),
        data_class: observed("internal".into()),
        host_capabilities: ModelRouteHostCapabilities {
            schema: tect_domain::MODEL_ROUTE_HOST_CAPABILITIES_SCHEMA.into(),
            version: 1,
            capabilities: vec!["owned-stdio".into()],
        }
        .fact()
        .unwrap(),
        remaining_budget_units: observed(20),
        available_latency_ms: observed(100),
    };
    let route = |name: &str, model: &str, effort: &str, enabled| ModelRoute {
        id: name.into(),
        provider: "openai".into(),
        model: model.into(),
        effort: effort.into(),
        enabled,
        allowed_matrix_choice_ids: vec!["choice-a".into()],
        allowed_roles: vec!["implementation".into()],
        allowed_tools: vec!["code".into()],
        allowed_data_classes: vec!["internal".into()],
        required_host_capabilities: vec!["owned-stdio".into()],
        minimum_budget_units: 10,
        minimum_latency_ms: 50,
    };
    let catalogue = ModelRouteCatalogue {
        schema: tect_domain::MODEL_ROUTE_CATALOGUE_SCHEMA.into(),
        version: 1,
        routes: vec![
            route("owner-luna6", "gpt-6-luna", "xhigh", true),
            route("owner-sol61", "gpt-6.1-sol", "medium", true),
            route("disabled-audit", "configured-disabled", "low", false),
        ],
    };
    let eligible = catalogue.eligible(&work).unwrap();
    let prepared = PreparedModelRouteRecommendation {
        workspace_id: id(1),
        request_key: "saved-prepare".into(),
        origin_session_id: Some(id(10)),
        session_preference: AdvisoryRequestPreference::UseWorkspace,
        request_preference: AdvisoryRequestPreference::UseWorkspace,
        advisory_config_revision: 2,
        work,
        catalogue: Some(catalogue),
        eligible: Some(eligible.clone()),
        preparation: ModelRoutePreparation::Prepared,
        routes: ModelRouteRecord {
            requested_route_id: Some("disabled-audit".into()),
            recommended_route_id: None,
            observed_actual: None,
        },
    };
    let decision = CapturedModelRouteDecision {
        id: id(11),
        prepared: prepared.clone(),
        input: ModelRouteDecisionInput::Ranking(ModelRouteRanking {
            catalogue_digest: eligible.catalogue_digest.clone(),
            work_context_digest: eligible.work_context_digest.clone(),
            ranked_route_ids: vec!["owner-luna6".into(), "owner-sol61".into()],
        }),
        outcome: ModelRouteDecisionOutcome::Recommended {
            route_id: "owner-luna6".into(),
        },
        routes: ModelRouteRecord {
            requested_route_id: prepared.routes.requested_route_id.clone(),
            recommended_route_id: Some("owner-luna6".into()),
            observed_actual: None,
        },
    };
    let disposition = CapturedModelRouteDisposition {
        id: id(12),
        decision_id: id(11),
        workspace_id: id(1),
        actor_id: id(2),
        action: ModelRouteDispositionAction::Accept,
        rationale: "synthetic accepted advice; explicit selected route differs".into(),
    };
    let input = EngineeringMatrixInput {
        mode: MatrixFact::Absent,
        envelope: OperatingEnvelope {
            scale: MatrixFact::Absent,
            operational_facts: OperationalFacts::Absent,
        },
        criticality: MatrixFact::Absent,
        intent: MatrixFact::Absent,
        urgency: MatrixFact::Absent,
        promised_behavior: MatrixFact::Absent,
        promised_proof: MatrixFact::Absent,
        affected_guarantees: MatrixFact::Absent,
        actual_exposure: MatrixFact::Absent,
        demand_commitment: MatrixFact::Absent,
        latency_commitment: MatrixFact::Absent,
        urgent_repair: MatrixFact::Absent,
    };
    let source = MatrixTaskSource {
        revision: MatrixTaskRevision {
            task_id: id(4),
            revision: 3,
            request_id: id(13),
            input,
            input_digest: "a".repeat(64),
            choice_set: None,
            choice_set_digest: Some("b".repeat(64)),
            recorded_by_principal_id: id(14),
            recorded_by_session_id: id(15),
        },
        requirements_binding: Some(MatrixTaskRequirementsBinding {
            locator: MatrixRequirementsLocator::Slice {
                program_id: id(16),
                scope_id: id(17),
                candidate_set_id: id(6),
                work_candidate_id: id(8),
                expected_work_revision: 2,
            },
            snapshot_id: id(9),
            semantic_digest: "d".repeat(64),
            authority_schema: tect_domain::MATRIX_REQUIREMENTS_SCHEMA.into(),
        }),
    };
    let link = MatrixPlanningSelectionLink {
        selection,
        context_provenance: Some(MatrixPlanningContextProvenance {
            frozen_snapshot_id: id(9),
            authority_schema: tect_domain::MATRIX_REQUIREMENTS_SCHEMA.into(),
            requirements_semantic_digest: "d".repeat(64),
        }),
        evaluation_digest: "f".repeat(64),
        catalogue_version: "EM02@0.1".into(),
        caller_principal_id: id(18),
        caller_session_id: id(19),
        scope_id: id(17),
        candidate_set_id: id(6),
        caller_request_id: id(7),
        result_revision: 4,
        mapped_nodes: vec![MatrixPlanningMappedNode {
            draft_index: 0,
            node_id: id(8),
            node_revision: 2,
        }],
    };
    Fixture {
        request: PrepareModelRouteHostSelection {
            preparation_request_key: "saved-prepare".into(),
            decision_id: id(11),
            disposition_id: id(12),
            expected_task_id: id(4),
            expected_task_revision: 3,
            expected_work_context_digest: eligible.work_context_digest,
            expected_catalogue_digest: eligible.catalogue_digest,
            selected_route_id: "owner-sol61".into(),
            input_sha256: "1".repeat(64),
            invocation_key: "synthetic-selection-key".into(),
        },
        prepared,
        decision,
        disposition,
        source,
        link,
    }
}
impl Fixture {
    fn material(&self) -> Result<ModelRouteHostSelectionMaterial> {
        selection_material(
            &self.request,
            id(1),
            id(2),
            id(3),
            AdvisoryRequestPreference::UseWorkspace,
            &self.prepared,
            &self.decision,
            &self.disposition,
            &self.source,
            &self.link,
            self.prepared.catalogue.as_ref(),
            &self.prepared.work.host_capabilities,
        )
    }
}

#[test]
fn explicit_selection_preserves_distinct_history_and_original_sessions() {
    let f = fixture();
    let material = f.material().unwrap();
    assert_eq!(material.selected_route.route_id, "owner-sol61");
    assert_eq!(material.selected_route.model, "gpt-6.1-sol");
    assert_eq!(material.selected_route.effort, "medium");
    assert_eq!(material.configured_route, material.selected_route);
    assert_eq!(material.requested_route.unwrap().route_id, "disabled-audit");
    assert_eq!(material.recommended_route.unwrap().route_id, "owner-luna6");
    assert_eq!(material.preparation, f.prepared);
    assert_eq!(material.decision, f.decision);
    assert_eq!(material.preparation.origin_session_id, Some(id(10)));
    assert_eq!(material.invoking_session_id, id(3));
    assert_eq!(
        material.source_binding.source_recorded_by_session_id,
        id(15)
    );
    assert_eq!(material.source_binding.matrix_save_session_id, id(19));
    assert!(material.decision.routes.observed_actual.is_none());
}

#[test]
fn current_selection_rejects_each_changed_identity_basis_and_history() {
    type Mutation = Box<dyn Fn(&mut Fixture)>;
    let mutations: Vec<Mutation> = vec![
        Box::new(|f| f.request.expected_task_id = id(88)),
        Box::new(|f| f.request.expected_task_revision += 1),
        Box::new(|f| f.request.expected_work_context_digest = "0".repeat(64)),
        Box::new(|f| f.request.expected_catalogue_digest = "0".repeat(64)),
        Box::new(|f| f.request.selected_route_id = "disabled-audit".into()),
        Box::new(|f| f.request.selected_route_id = "missing".into()),
        Box::new(|f| f.disposition.actor_id = id(88)),
        Box::new(|f| f.disposition.workspace_id = id(88)),
        Box::new(|f| f.disposition.action = ModelRouteDispositionAction::Reject),
        Box::new(|f| f.disposition.decision_id = id(88)),
        Box::new(|f| f.source.requirements_binding = None),
        Box::new(|f| f.source.revision.input_digest = "0".repeat(64)),
        Box::new(|f| f.source.revision.revision += 1),
        Box::new(|f| {
            f.source
                .requirements_binding
                .as_mut()
                .unwrap()
                .semantic_digest = "0".repeat(64)
        }),
        Box::new(|f| f.link.caller_request_id = id(88)),
        Box::new(|f| f.link.mapped_nodes[0].node_revision += 1),
        Box::new(|f| f.prepared.origin_session_id = None),
        Box::new(|f| f.prepared.session_preference = AdvisoryRequestPreference::Skip),
        Box::new(|f| {
            f.decision.outcome = ModelRouteDecisionOutcome::Abstained {
                reason: tect_domain::ModelRouteAbstainReason::Explicit,
            }
        }),
        Box::new(|f| f.decision.routes.recommended_route_id = Some("owner-sol61".into())),
        Box::new(|f| f.prepared.work.remaining_budget_units = ModelRouteFact::Unknown),
    ];
    for (index, mutate) in mutations.into_iter().enumerate() {
        let mut f = fixture();
        mutate(&mut f);
        assert!(f.material().is_err(), "mutation {index}");
    }
}

#[test]
fn selection_rejects_current_catalogue_capability_and_preference_changes() {
    let f = fixture();
    let mut catalogue = f.prepared.catalogue.clone().unwrap();
    catalogue.version += 1;
    for (catalogue, host, preference) in [
        (
            Some(&catalogue),
            &f.prepared.work.host_capabilities,
            AdvisoryRequestPreference::UseWorkspace,
        ),
        (
            f.prepared.catalogue.as_ref(),
            &ModelRouteFact::Unknown,
            AdvisoryRequestPreference::UseWorkspace,
        ),
        (
            f.prepared.catalogue.as_ref(),
            &f.prepared.work.host_capabilities,
            AdvisoryRequestPreference::Skip,
        ),
    ] {
        assert!(
            selection_material(
                &f.request,
                id(1),
                id(2),
                id(3),
                preference,
                &f.prepared,
                &f.decision,
                &f.disposition,
                &f.source,
                &f.link,
                catalogue,
                host
            )
            .is_err()
        );
    }
}

#[test]
fn material_digest_covers_input_key_and_full_nested_history() {
    let material = fixture().material().unwrap();
    let original = material.canonical_digest().unwrap();
    for mutation in [0, 1, 2, 3, 4, 5] {
        let mut changed = material.clone();
        match mutation {
            0 => changed.input_sha256 = "2".repeat(64),
            1 => changed.invocation_key.push('x'),
            2 => changed.disposition.rationale.push('x'),
            3 => changed.source_binding.matrix_save_session_id = id(88),
            4 => changed.configured_route.effort = "high".into(),
            _ => changed.preparation.advisory_config_revision += 1,
        };
        assert_ne!(changed.canonical_digest().unwrap(), original);
    }
    let encoded = material.canonical_json().unwrap();
    assert!(!encoded.contains("credential"));
    assert!(!encoded.contains("native_session_id"));
    assert_eq!(
        material.canonical_digest().unwrap(),
        format!("{:x}", sha2::Sha256::digest(encoded.as_bytes()))
    );
}

#[test]
fn synthetic_selection_golden_encoding() {
    let material = fixture().material().unwrap();
    // Synthetic contract data only; these UUIDs are not genuine approval.
    assert_eq!(
        material.canonical_json().unwrap(),
        include_str!("synthetic_selection_golden.json").trim_end()
    );
    assert_eq!(
        material.canonical_digest().unwrap(),
        "6b56a713aa759f56d7e51016ff4e3f5e001802551657741e25f81f4fb8058598"
    );
}

#[test]
fn missing_original_bound_locator_is_an_input_conflict() {
    let mut f = fixture();
    f.source.requirements_binding = None;
    assert_eq!(f.material(), Err(Error::InputConflict));
}

#[test]
fn absent_requested_route_stays_absent_with_explicit_selection() {
    let mut f = fixture();
    f.prepared.routes.requested_route_id = None;
    f.decision.prepared = f.prepared.clone();
    f.decision.routes.requested_route_id = None;
    let material = f.material().unwrap();
    assert!(material.requested_route.is_none());
    assert!(material.preparation.routes.requested_route_id.is_none());
    assert_eq!(material.selected_route.route_id, "owner-sol61");
}

#[test]
fn malformed_request_pins_fail_before_selection_reads() {
    let mut request = fixture().request;
    request.input_sha256 = "A".repeat(64);
    assert_eq!(validate_request(&request), Err(Error::InvalidArguments));
    request = fixture().request;
    request.invocation_key = " padded ".into();
    assert_eq!(validate_request(&request), Err(Error::InvalidArguments));
    request = fixture().request;
    request.decision_id = Uuid::nil();
    assert_eq!(validate_request(&request), Err(Error::InvalidArguments));
}

fn change_catalogue_provider(f: &mut Fixture, route_id: &str) {
    let catalogue = f.prepared.catalogue.as_mut().unwrap();
    catalogue
        .routes
        .iter_mut()
        .find(|route| route.id == route_id)
        .unwrap()
        .provider = "other-provider".into();
    let eligible = catalogue.eligible(&f.prepared.work).unwrap();
    assert!(eligible.route_ids.contains(&f.request.selected_route_id));
    f.request.expected_catalogue_digest = eligible.catalogue_digest.clone();
    f.prepared.eligible = Some(eligible.clone());
    f.decision.prepared = f.prepared.clone();
    let ModelRouteDecisionInput::Ranking(ranking) = &mut f.decision.input else {
        panic!()
    };
    ranking.catalogue_digest = eligible.catalogue_digest;
}

#[test]
fn owned_stdio_selection_rejects_otherwise_current_eligible_non_openai_provider() {
    let mut f = fixture();
    let selected_id = f.request.selected_route_id.clone();
    change_catalogue_provider(&mut f, &selected_id);
    assert_eq!(f.material(), Err(Error::InputConflict));
}

#[test]
fn other_provider_audit_dimensions_do_not_change_openai_selection() {
    let mut f = fixture();
    change_catalogue_provider(&mut f, "disabled-audit");
    change_catalogue_provider(&mut f, "owner-luna6");
    let material = f.material().unwrap();
    assert_eq!(material.requested_route.unwrap().provider, "other-provider");
    assert_eq!(
        material.recommended_route.unwrap().provider,
        "other-provider"
    );
    assert_eq!(material.selected_route.provider, "openai");
    assert_eq!(material.configured_route.provider, "openai");
}

mod live_route_input_gate;
