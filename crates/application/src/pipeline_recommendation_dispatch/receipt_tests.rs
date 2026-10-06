#[test]
fn send_retains_and_accounts_before_post_send_authorization() {
    let send = include_str!("../pipeline_recommendation_dispatch.rs");
    let start = send
        .find("tx.commit().await?;\n        let permit")
        .unwrap();
    let observe = send.find(".observe_prepared(").unwrap();
    let seal = send.find(".seal_committed_advisory_observation(").unwrap();
    let usage = send.find(".usage_from_sealed_response(").unwrap();
    let consume = send
        .find(".consume_committed_advisory_observation(")
        .unwrap();
    let finish = send.find(".finish_pipeline_receipt(").unwrap();
    assert!(
        start < observe && observe < seal && seal < usage && usage < consume && consume < finish
    );
    assert!(!send[observe..finish].contains("self.authorized("));
}

#[test]
fn recovery_has_no_transport_and_terminal_replay_precedes_consumer() {
    let receipt = include_str!("receipts.rs");
    let recover = &receipt[..receipt.find("async fn replay_pipeline_receipt").unwrap()];
    assert!(!recover.contains("observe_prepared"));
    assert!(!recover.contains("start_pipeline_dispatch"));
    assert!(
        recover.find(".replay_pipeline_receipt(").unwrap()
            < recover
                .find(".consume_committed_advisory_observation(")
                .unwrap()
    );
    let replay = &receipt[receipt.find("async fn replay_pipeline_receipt").unwrap()
        ..receipt
            .find("pub(super) async fn finish_pipeline_receipt")
            .unwrap()];
    assert!(replay.find("Some(value)").unwrap() < replay.find("None =>").unwrap());
    assert!(!replay.contains("consume_committed"));
}

#[test]
fn advice_is_persisted_atomically_after_current_gates() {
    let receipt = include_str!("receipts.rs");
    let finish = &receipt[receipt
        .find("pub(super) async fn finish_pipeline_receipt")
        .unwrap()..];
    let current = finish.find("if !current").unwrap();
    let parse = finish.find(".parse_sealed_response(").unwrap();
    let insert = finish
        .find(".insert_pipeline_advice_interpretation(")
        .unwrap();
    let finalize = finish.find("tx.finalize_advisory_opportunity(").unwrap();
    let committed = finish[finalize..].find("tx.commit().await?").unwrap() + finalize;
    assert!(current < parse && parse < insert && insert < finalize && finalize < committed);
    assert!(!finish[insert..committed].contains(".authorized("));
}
