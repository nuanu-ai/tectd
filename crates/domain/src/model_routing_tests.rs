use super::*;
use uuid::Uuid;

fn route(id: &str) -> ModelRoute {
    ModelRoute {
        id: id.into(),
        provider: "configured-provider".into(),
        model: "configured-model".into(),
        effort: "configured-effort".into(),
        enabled: true,
        allowed_matrix_choice_ids: vec!["choice-a".into()],
        allowed_roles: vec!["agent".into()],
        allowed_tools: vec!["code".into()],
        allowed_data_classes: vec!["internal".into()],
        required_host_capabilities: vec!["model-api".into()],
        minimum_budget_units: 10,
        minimum_latency_ms: 50,
    }
}

fn catalogue() -> ModelRouteCatalogue {
    ModelRouteCatalogue {
        schema: MODEL_ROUTE_CATALOGUE_SCHEMA.into(),
        version: 1,
        routes: vec![route("route-a"), route("route-b")],
    }
}

fn work() -> ModelRouteWorkContext {
    ModelRouteWorkContext {
        approved_matrix_selection: MatrixPlanningSelection {
            task_id: Uuid::new_v4(),
            task_revision: 3,
            disposition_id: Uuid::new_v4(),
            selected_choice_id: "choice-a".into(),
            expected_input_digest: "a".repeat(64),
            expected_choice_set_digest: "b".repeat(64),
            expected_verification_digest: "c".repeat(64),
            mapped_draft_node_indices: vec![0],
        },
        selection_link: ModelRouteSelectionLink {
            candidate_set_id: Uuid::new_v4(),
            caller_request_id: Uuid::new_v4(),
            mapped_draft_node_index: 0,
            mapped_work_node_id: Uuid::from_u128(1),
            mapped_work_node_revision: 1,
        },
        role: caller("agent".into()),
        tool: caller("code".into()),
        data_class: caller("internal".into()),
        host_capabilities: ModelRouteFact::Known {
            value: vec!["model-api".into()],
            provenance: ModelRouteFactProvenance::Host {
                evidence_ref: "host/capabilities/1".into(),
            },
        },
        remaining_budget_units: caller(10),
        available_latency_ms: caller(50),
    }
}

fn caller<T>(value: T) -> ModelRouteFact<T> {
    ModelRouteFact::Known {
        value,
        provenance: ModelRouteFactProvenance::Caller {
            source_ref: "work/node/1".into(),
            work_node_id: Uuid::from_u128(1),
            work_node_revision: 1,
        },
    }
}

#[test]
fn policy_constraints_exclude_every_disallowed_route() {
    let mut routes = vec![route("allowed")];
    let mut disabled = route("disabled");
    disabled.enabled = false;
    routes.push(disabled);
    let mut wrong_role = route("wrong-role");
    wrong_role.allowed_roles = vec!["owner".into()];
    routes.push(wrong_role);
    let mut wrong_matrix_choice = route("wrong-matrix-choice");
    wrong_matrix_choice.allowed_matrix_choice_ids = vec!["choice-b".into()];
    routes.push(wrong_matrix_choice);
    let mut wrong_tool = route("wrong-tool");
    wrong_tool.allowed_tools = vec!["search".into()];
    routes.push(wrong_tool);
    let mut wrong_class = route("wrong-class");
    wrong_class.allowed_data_classes = vec!["public".into()];
    routes.push(wrong_class);
    let mut wrong_host = route("wrong-host");
    wrong_host.required_host_capabilities.push("gpu".into());
    routes.push(wrong_host);
    let mut expensive = route("expensive");
    expensive.minimum_budget_units = 11;
    routes.push(expensive);
    let mut slow = route("slow");
    slow.minimum_latency_ms = 51;
    routes.push(slow);
    let catalogue = ModelRouteCatalogue {
        schema: MODEL_ROUTE_CATALOGUE_SCHEMA.into(),
        version: 1,
        routes,
    };
    let eligible = catalogue.eligible(&work()).unwrap();
    assert_eq!(eligible.route_ids, vec!["allowed"]);
}

#[test]
fn catalogue_is_versioned_digested_and_rejects_duplicates() {
    let base = catalogue();
    let digest = base.digest().unwrap();
    let mut reordered = base.clone();
    reordered.routes.reverse();
    assert_eq!(reordered.digest().unwrap(), digest);
    reordered.version = 2;
    assert_ne!(reordered.digest().unwrap(), digest);
    reordered = base.clone();
    reordered.routes.push(route("route-a"));
    assert_eq!(reordered.validate(), Err(Error::InvalidArguments));
    reordered = base.clone();
    reordered.routes[0].allowed_roles.push("agent".into());
    assert_eq!(reordered.validate(), Err(Error::InvalidArguments));
    reordered = base.clone();
    reordered.routes[0].allowed_matrix_choice_ids = vec!["choice-b".into()];
    assert_ne!(reordered.digest().unwrap(), digest);
    reordered = base.clone();
    reordered.routes[0]
        .allowed_matrix_choice_ids
        .push("choice-a".into());
    assert_eq!(reordered.validate(), Err(Error::InvalidArguments));
    reordered = base;
    reordered.routes[0].provider.clear();
    assert_eq!(reordered.validate(), Err(Error::InvalidArguments));
}

#[test]
fn ranking_rejects_unknown_duplicate_and_stale_ids() {
    let eligible = catalogue().eligible(&work()).unwrap();
    let mut ranking = ModelRouteRanking {
        catalogue_digest: eligible.catalogue_digest.clone(),
        work_context_digest: eligible.work_context_digest.clone(),
        ranked_route_ids: vec!["route-b".into(), "route-a".into()],
    };
    assert_eq!(
        eligible.recommendation(&ranking).unwrap(),
        Some("route-b".into())
    );
    ranking.ranked_route_ids.push("route-b".into());
    assert_eq!(
        eligible.recommendation(&ranking),
        Err(Error::InvalidArguments)
    );
    ranking.ranked_route_ids = vec!["unknown".into()];
    assert_eq!(
        eligible.recommendation(&ranking),
        Err(Error::InvalidArguments)
    );
    ranking.ranked_route_ids.clear();
    ranking.catalogue_digest = "0".repeat(64);
    assert_eq!(eligible.recommendation(&ranking), Err(Error::StaleRevision));
    ranking.catalogue_digest = eligible.catalogue_digest.clone();
    ranking.work_context_digest = "0".repeat(64);
    assert_eq!(eligible.recommendation(&ranking), Err(Error::StaleRevision));
}

#[test]
fn exact_matrix_and_work_facts_change_binding() {
    let original = work();
    let digest = original.digest().unwrap();
    let mut changed = original.clone();
    changed.approved_matrix_selection.task_revision += 1;
    assert_ne!(changed.digest().unwrap(), digest);
    changed = original.clone();
    changed.remaining_budget_units = caller(11);
    assert_ne!(changed.digest().unwrap(), digest);
    changed = original;
    changed
        .approved_matrix_selection
        .expected_verification_digest
        .clear();
    assert_eq!(changed.digest(), Err(Error::InvalidArguments));
}

#[test]
fn exact_approved_matrix_choice_gates_eligibility() {
    let original = work();
    let catalogue = catalogue();
    assert_eq!(catalogue.eligible(&original).unwrap().route_ids.len(), 2);
    let mut changed = original.clone();
    changed.approved_matrix_selection.selected_choice_id = "choice-b".into();
    assert!(catalogue.eligible(&changed).unwrap().route_ids.is_empty());
    changed.approved_matrix_selection.selected_choice_id = "choice-a".into();
    changed
        .approved_matrix_selection
        .expected_input_digest
        .clear();
    assert_eq!(catalogue.eligible(&changed), Err(Error::InvalidArguments));
}

#[test]
fn no_route_is_abstention_and_has_no_execution_action() {
    let mut no_budget = work();
    no_budget.remaining_budget_units = caller(0);
    let eligible = catalogue().eligible(&no_budget).unwrap();
    assert!(eligible.route_ids.is_empty());
    let ranking = ModelRouteRanking {
        catalogue_digest: eligible.catalogue_digest.clone(),
        work_context_digest: eligible.work_context_digest.clone(),
        ranked_route_ids: vec![],
    };
    assert_eq!(eligible.recommendation(&ranking).unwrap(), None);
    assert_eq!(
        eligible.record(None, None, None).unwrap().observed_actual,
        None
    );
    assert_eq!(eligible.configured_route_ids, vec!["route-a", "route-b"]);
    assert_eq!(
        eligible
            .record(Some("route-a".into()), None, None)
            .unwrap()
            .requested_route_id
            .as_deref(),
        Some("route-a")
    );
}

#[test]
fn unknown_facts_fail_closed_and_have_distinct_digest() {
    let base = work();
    let mut unknown = base.clone();
    unknown.remaining_budget_units = ModelRouteFact::Unknown;
    assert!(unknown.has_unknown_facts());
    assert!(catalogue().eligible(&unknown).unwrap().route_ids.is_empty());
    assert_ne!(unknown.digest().unwrap(), base.digest().unwrap());
    let mut bad = base.clone();
    bad.host_capabilities = caller(vec!["model-api".into()]);
    assert_eq!(bad.digest(), Err(Error::InvalidArguments));
    bad = base;
    bad.role = ModelRouteFact::Known {
        value: "agent".into(),
        provenance: ModelRouteFactProvenance::Caller {
            source_ref: "work/node/other".into(),
            work_node_id: Uuid::new_v4(),
            work_node_revision: 1,
        },
    };
    assert_eq!(bad.digest(), Err(Error::InvalidArguments));
}

#[test]
fn mapped_work_node_and_receipt_are_required_and_bound() {
    let base = work();
    let mut changed = base.clone();
    changed.selection_link.caller_request_id = Uuid::new_v4();
    assert_ne!(changed.digest().unwrap(), base.digest().unwrap());
    changed = base.clone();
    changed.selection_link.mapped_work_node_revision += 1;
    assert_eq!(changed.digest(), Err(Error::InvalidArguments));
    changed.selection_link.mapped_draft_node_index = 1;
    assert_eq!(changed.digest(), Err(Error::InvalidArguments));
}

#[test]
fn requested_recommended_and_observed_actual_remain_distinct() {
    let eligible = catalogue().eligible(&work()).unwrap();
    let record = eligible
        .record(
            Some("route-a".into()),
            Some("route-b".into()),
            Some(ObservedModelRoute {
                route_id: None,
                provider: "observed-provider".into(),
                model: "observed-model".into(),
                effort: "observed-effort".into(),
                evidence_ref: "host-run:123".into(),
            }),
        )
        .unwrap();
    assert_eq!(record.requested_route_id.as_deref(), Some("route-a"));
    assert_eq!(record.recommended_route_id.as_deref(), Some("route-b"));
    assert_eq!(record.observed_actual.unwrap().model, "observed-model");
    assert_eq!(
        eligible
            .record(None, Some("route-a".into()), None)
            .unwrap()
            .observed_actual,
        None
    );
    assert_eq!(
        eligible.record(None, Some("missing".into()), None),
        Err(Error::InvalidArguments)
    );
    assert_eq!(
        eligible.record(Some("missing".into()), None, None),
        Err(Error::InvalidArguments)
    );
}
