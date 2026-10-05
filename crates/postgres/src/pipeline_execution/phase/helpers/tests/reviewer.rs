use super::*;

fn reviewer_fixture(
    fresh: bool,
    independent: bool,
    context: bool,
) -> (PipelinePhaseDefinition, PipelinePhaseOutputDraft) {
    let mut phase = navigation_phase("K3.7", 7, &[]);
    phase.fresh_reviewer_input = fresh;
    phase
        .output_constraints
        .push(PipelineOutputConstraint::ReviewerContextMode {
            field: "review_mode".into(),
            independent_value: "independent".into(),
            self_value: "self".into(),
        });
    let mut output = waiting_request(None).output;
    output.producer_context_id = "new-review-label".into();
    output.fields.insert(
        "review_mode".into(),
        if independent { "independent" } else { "self" }.into(),
    );
    if context {
        output.reviewer_context = Some(PipelineReviewerAttestation {
            reviewer_identity: "changed caller identity".into(),
            reviewer_context_id: "new-review-label".into(),
            producer_context_ids: vec!["prior-label".into()],
            fresh_input: true,
        });
    }
    (phase, output)
}

fn producer_row(actor: Uuid) -> (Option<String>, Option<Uuid>) {
    (Some("prior-label".into()), Some(actor))
}

fn assert_reviewer_refusal(error: Error, rule: &str, path: &str, forbidden: Uuid) {
    let Error::Refused(refusal) = &error else {
        panic!("expected typed reviewer refusal");
    };
    let json = serde_json::to_value(refusal).unwrap();
    assert_eq!(json["rule"], rule);
    assert_eq!(json["path"], path);
    assert!(
        !serde_json::to_string(&error)
            .unwrap()
            .contains(&forbidden.to_string())
    );
    assert!(!error.to_string().contains(&forbidden.to_string()));
}

#[test]
fn reviewer_fresh_changed_labels_do_not_hide_same_authenticated_actor() {
    let actor = Uuid::new_v4();
    let (phase, output) = reviewer_fixture(true, false, true);
    assert_reviewer_refusal(
        validate_reviewer_provenance(actor, &phase, &output, &[producer_row(actor)]).unwrap_err(),
        "WP6-REVIEW-INDEPENDENCE-01",
        "arguments.params.output.reviewer_context",
        actor,
    );
}

#[test]
fn reviewer_k37_independent_mode_enforces_native_boundary_when_fresh_false() {
    let actor = Uuid::new_v4();
    let (phase, output) = reviewer_fixture(false, true, true);
    assert_reviewer_refusal(
        validate_reviewer_provenance(actor, &phase, &output, &[producer_row(actor)]).unwrap_err(),
        "WP6-REVIEW-INDEPENDENCE-01",
        "arguments.params.output.fields.review_mode",
        actor,
    );
}

#[test]
fn reviewer_optional_context_enforces_native_boundary() {
    let actor = Uuid::new_v4();
    let (mut phase, output) = reviewer_fixture(false, false, true);
    phase.output_constraints.clear();
    assert_reviewer_refusal(
        validate_reviewer_provenance(actor, &phase, &output, &[producer_row(actor)]).unwrap_err(),
        "WP6-REVIEW-INDEPENDENCE-01",
        "arguments.params.output.reviewer_context",
        actor,
    );
}

#[test]
fn reviewer_different_native_actor_allowed_without_principal_comparison() {
    // Boundary deliberately accepts native actors, not principal IDs. Two
    // authenticated sessions owned by one principal may review independently.
    let (phase, output) = reviewer_fixture(true, true, true);
    assert!(
        validate_reviewer_provenance(
            Uuid::new_v4(),
            &phase,
            &output,
            &[producer_row(Uuid::new_v4())]
        )
        .is_ok()
    );
}

#[test]
fn reviewer_any_equal_actor_among_multiple_producers_refused() {
    let actor = Uuid::new_v4();
    let (phase, output) = reviewer_fixture(true, true, true);
    assert!(
        validate_reviewer_provenance(
            actor,
            &phase,
            &output,
            &[producer_row(Uuid::new_v4()), producer_row(actor)]
        )
        .is_err()
    );
}

#[test]
fn reviewer_legal_self_without_context_needs_no_provenance() {
    let (phase, output) = reviewer_fixture(false, false, false);
    assert!(validate_reviewer_provenance(Uuid::new_v4(), &phase, &output, &[]).is_ok());
}

#[test]
fn reviewer_fresh_true_cannot_bypass_boundary_with_self_mode_and_no_context() {
    let actor = Uuid::new_v4();
    let (phase, output) = reviewer_fixture(true, false, false);
    assert_reviewer_refusal(
        validate_reviewer_provenance(actor, &phase, &output, &[producer_row(actor)]).unwrap_err(),
        "WP6-REVIEW-INDEPENDENCE-01",
        "arguments.params.output.reviewer_context",
        actor,
    );
}

#[test]
fn reviewer_missing_or_erased_provenance_fails_closed() {
    let actor = Uuid::new_v4();
    let (phase, output) = reviewer_fixture(true, true, true);
    for rows in [
        vec![],
        vec![(None, None)],
        vec![(Some("prior-label".into()), None)],
        vec![(None, Some(Uuid::new_v4()))],
    ] {
        assert_reviewer_refusal(
            validate_reviewer_provenance(actor, &phase, &output, &rows).unwrap_err(),
            "WP6-REVIEW-PROVENANCE-01",
            "arguments.params.output.fields.review_mode",
            actor,
        );
    }
}

#[test]
fn reviewer_existing_label_consistency_checks_retained() {
    let (phase, output) = reviewer_fixture(true, true, true);
    for mutation in 0..4 {
        let mut output = output.clone();
        let ctx = output.reviewer_context.as_mut().unwrap();
        match mutation {
            0 => ctx.reviewer_context_id = "mismatch".into(),
            1 => ctx.producer_context_ids = vec!["wrong".into()],
            2 => ctx.producer_context_ids.push("prior-label".into()),
            _ => {
                output.producer_context_id = "prior-label".into();
                ctx.reviewer_context_id = "prior-label".into();
            }
        }
        let error = validate_reviewer_provenance(
            Uuid::new_v4(),
            &phase,
            &output,
            &[producer_row(Uuid::new_v4())],
        )
        .unwrap_err();
        let refusal = error.refusal().unwrap();
        assert_eq!(refusal.code, RefusalCode::InvalidOutput);
        let (rule, field) = match mutation {
            0 => ("WP6-REVIEW-CONTEXT-05", "reviewer_context_id"),
            1 => ("WP6-REVIEW-PRODUCERS-07", "producer_context_ids"),
            2 => ("WP6-REVIEW-PRODUCERS-06", "producer_context_ids"),
            _ => ("WP6-REVIEW-PRODUCERS-05", "producer_context_ids"),
        };
        assert_eq!(refusal.rule.as_deref(), Some(rule));
        assert_eq!(
            refusal.path.unwrap(),
            format!("arguments.params.output.reviewer_context.{field}")
        );
    }
}

#[test]
fn reviewer_provenance_query_scopes_current_bindings_and_historical_actors() {
    // Source assertion only: executing the scoped joins requires a disposable
    // PostgreSQL fixture; this does not claim database integration coverage.
    let source = include_str!("../validation.rs");
    let query = source
        .split("const REVIEW_PRODUCER_PROVENANCE_SQL: &str =")
        .nth(1)
        .unwrap()
        .split(';')
        .next()
        .unwrap();
    for clause in [
        "b.tenant_id=$1",
        "b.workspace_id=$2",
        "b.run_id=$3",
        "b.phase_ordinal<$4",
        "b.stale=false",
        "o.tenant_id=b.tenant_id",
        "o.workspace_id=b.workspace_id",
        "o.run_id=b.run_id",
        "o.id=b.output_id",
        "NOT o.payload_erased",
        "a.tenant_id=o.tenant_id",
        "a.workspace_id=o.workspace_id",
        "a.run_id=o.run_id",
        "a.id=o.attempt_id",
        "NOT a.payload_erased",
        "a.actor_session_id",
    ] {
        assert!(
            query.contains(clause),
            "missing scoped provenance clause: {clause}"
        );
    }
    assert!(query.contains("LEFT JOIN slice_pipeline_phase_outputs"));
    assert!(query.contains("LEFT JOIN slice_pipeline_phase_attempts"));
    assert!(!query.contains("agent_sessions"));
    assert!(!query.contains("revoked"));
}
