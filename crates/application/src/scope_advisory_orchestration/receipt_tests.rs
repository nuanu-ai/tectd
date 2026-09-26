#[test]
fn scope_raw_accounting_precedes_every_current_gate_and_ranking() {
    let send = include_str!("dispatch.rs");
    let finish = include_str!("receipts.rs");
    let observe = send.find(".observe_prepared(").unwrap();
    let seal = send.find(".seal_committed_advisory_observation(").unwrap();
    let usage = send.find(".usage_from_sealed_response(").unwrap();
    let consume = send
        .find(".consume_committed_advisory_observation(")
        .unwrap();
    let finish_call = send.find(".finish_scope_receipt(").unwrap();
    assert!(observe < seal && seal < usage && usage < consume && consume < finish_call);
    assert!(!send[observe..finish_call].contains(".scope_transaction("));
    assert!(!send[observe..finish_call].contains("guard_scope_advice("));
    assert!(send[usage..consume].contains("unwrap_or_default()"));
    assert!(send.contains("let sealed_prepared = prepared_attempt.clone()"));
    assert!(!send.contains("send_permit.clone()"));
    let parse = finish.find(".parse_sealed_response(").unwrap();
    for gate in [
        "self.authorized(",
        "consumption.unknown_usage",
        "config_current",
        "provider_current",
        "fresh_manifest",
        "lookup_verified_scope_budget(",
        "budget_current",
    ] {
        assert!(finish.find(gate).unwrap() < parse, "{gate}");
    }
    assert!(finish.contains(".finalize_scope_advisory_without_advice("));
    assert!(!finish.contains("seal_advisory_dispatch"));
}

#[test]
fn lawful_saved_recovery_accounts_before_current_source_and_never_resends() {
    let run = include_str!("run.rs");
    assert!(
        run.find("self.authorized(").unwrap() < run.find(".scope_receipt_for_replay(").unwrap()
    );
    assert!(
        run.find(".scope_receipt_for_replay(").unwrap()
            < run.find("early_no_call_target(").unwrap()
    );
    let recovery = include_str!("recovery.rs");
    assert!(recovery.contains("saved.opportunity.authorized_actor_id != actor"));
    assert!(!recovery.contains("opportunity.session_id"));
    assert!(!recovery.contains("start_advisory_dispatch"));
    assert!(!recovery.contains("observe_prepared"));
    let compact: String = recovery.chars().filter(|ch| !ch.is_whitespace()).collect();
    assert!(compact.contains("saved.dispatch.input_tokens"));
    assert!(recovery.contains("saved.request_payload.clone()"));
    assert!(
        recovery
            .find(".consume_committed_advisory_observation(")
            .unwrap()
            < recovery
                .find("let prepared = prepared_from_receipt")
                .unwrap()
    );
    assert!(compact.contains("&stored.record.manifest,None"));
}
