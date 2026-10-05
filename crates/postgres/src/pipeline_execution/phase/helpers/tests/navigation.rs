use super::*;

#[test]
fn waiting_rework_moves_to_the_exact_backward_phase() {
    let definition = navigation_definition(&["K2"]);
    let phase = &definition.phases[2];
    let plan = plan_next_state(&waiting_request(Some("K2")), &definition, phase).unwrap();

    assert_eq!(plan.status, "active");
    assert_eq!(plan.next_id.as_deref(), Some("K2"));
    assert_eq!(plan.next_ordinal, Some(2));
    assert_eq!(plan.revisit_ordinal, Some(2));
}

#[test]
fn waiting_without_rework_stays_on_the_current_phase() {
    let definition = navigation_definition(&["K2"]);
    let phase = &definition.phases[2];
    let plan = plan_next_state(&waiting_request(None), &definition, phase).unwrap();

    assert_eq!(plan.status, "waiting_input");
    assert_eq!(plan.next_id.as_deref(), Some("K3"));
    assert_eq!(plan.next_ordinal, Some(3));
    assert_eq!(plan.revisit_ordinal, None);
}

#[test]
fn waiting_rework_rejects_missing_disallowed_and_forward_targets() {
    let missing_definition = navigation_definition(&["missing"]);
    assert!(matches!(
        plan_next_state(
            &waiting_request(Some("missing")),
            &missing_definition,
            &missing_definition.phases[2],
        ),
        Err(Error::InvalidArguments)
    ));

    let disallowed_definition = navigation_definition(&["K2"]);
    assert!(matches!(
        plan_next_state(
            &waiting_request(Some("K1")),
            &disallowed_definition,
            &disallowed_definition.phases[2],
        ),
        Err(Error::Forbidden)
    ));

    let mut forward_definition = navigation_definition(&["K3"]);
    forward_definition
        .phases
        .push(navigation_phase("K4", 4, &[]));
    forward_definition.phases[2].allowed_backward_to = vec!["K4".into()];
    assert!(matches!(
        plan_next_state(
            &waiting_request(Some("K4")),
            &forward_definition,
            &forward_definition.phases[2],
        ),
        Err(Error::Forbidden)
    ));
}
