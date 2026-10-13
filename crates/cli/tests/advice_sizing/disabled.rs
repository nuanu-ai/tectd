//! Disabled means `main`: with no setting, or with a setting that says disabled, a
//! draft save answers without an `advice` field, and a Slice draft save also without
//! a `pipeline_advice` field, writes no advice row and calls nobody.
use crate::drafts::{Level, Shape, target};
use crate::support::{Jev, Stand, command, configure};
use uuid::Uuid;

fn keeping(name: &str) -> (&'static str, f64) {
    match name.rsplit('.').next() {
        Some("size") => ("bounded", 0.7),
        Some("advice") => ("keep", 0.7),
        // The second point of a Slice draft save; what it answers is its own suite's.
        Some("pipeline") => ("ABSTAIN", 0.7),
        _ => ("requested", 0.7),
    }
}

async fn a_disabled_jev_answers_as_main_does(level: Level) {
    let stand = Stand::start("2000").await;
    stand.jev.answer(Jev::Answers(keeping));
    let (mut client, repo) = stand.workspace().await;
    let target_one = target(&mut client, &repo, level).await;
    let params = target_one.save(&Shape::of(2, 2), Uuid::new_v4());
    let route = level.save_route();

    // No setting row: the state of every workspace on `main`.
    let (plain, plain_bytes) = command(&mut client, route, params.clone()).await;
    assert!(plain.get("advice").is_none(), "{plain}");
    assert!(plain.get("pipeline_advice").is_none(), "{plain}");
    assert_eq!(stand.count("advice_config").await, 0);
    assert_eq!(stand.count("advice_journal").await, 0);

    // A setting that says disabled: the replay is the same reply, byte for byte.
    configure(&mut client, false, 0).await;
    let (disabled, disabled_bytes) = command(&mut client, route, params.clone()).await;
    assert_eq!(disabled, plain);
    assert_eq!(disabled_bytes, plain_bytes);
    assert_eq!(stand.count("advice_journal").await, 0);
    assert_eq!(stand.jev.calls(), 0);

    // Enabled: the same reply with one more field per advice point of the command,
    // and one journal row and one call for `sizing`.
    configure(&mut client, true, 1).await;
    let (enabled, _) = command(&mut client, route, params.clone()).await;
    assert_eq!(enabled["advice"]["status"], "advised");
    let mut added = vec!["advice"];
    if level == Level::Slices {
        added.push("pipeline_advice");
    }
    let mut without = enabled.clone();
    for field in &added {
        assert!(
            without.as_object_mut().unwrap().remove(*field).is_some(),
            "{field}"
        );
    }
    assert_eq!(without, plain);
    assert_eq!(stand.jev.asked(".size").len(), 1);
    assert_eq!(stand.rows_of("sizing").await, (1, 1));
    let written = (stand.jev.calls(), stand.count("advice_journal").await);
    assert_eq!(written, (added.len(), added.len() as i64));

    // Disabled again: no stored field is shown and nothing more is written.
    configure(&mut client, false, 2).await;
    let (again, again_bytes) = command(&mut client, route, params).await;
    assert_eq!(again, plain);
    assert_eq!(again_bytes, plain_bytes);
    assert_eq!(
        (stand.jev.calls(), stand.count("advice_journal").await),
        written
    );

    // A first save, not a replay, in a workspace whose setting says disabled.
    let (mut other, repo) = stand.workspace().await;
    let target_two = target(&mut other, &repo, level).await;
    configure(&mut other, false, 0).await;
    let params = target_two.save(&Shape::of(2, 2), Uuid::new_v4());
    let (fresh, _) = command(&mut other, route, params).await;
    assert!(fresh.get("advice").is_none(), "{fresh}");
    assert_eq!(
        fresh.as_object().unwrap().keys().collect::<Vec<_>>(),
        plain.as_object().unwrap().keys().collect::<Vec<_>>()
    );
    assert_eq!(
        (stand.jev.calls(), stand.count("advice_journal").await),
        written
    );
    client.finish().await;
    other.finish().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn scope_draft_save_with_jev_disabled_answers_as_main_does() {
    a_disabled_jev_answers_as_main_does(Level::Scopes).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn slice_draft_save_with_jev_disabled_answers_as_main_does() {
    a_disabled_jev_answers_as_main_does(Level::Slices).await;
}
