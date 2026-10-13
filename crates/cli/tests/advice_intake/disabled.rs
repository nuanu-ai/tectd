//! Disabled means `main`: with no setting, or with a setting that says disabled, a
//! recorded input answers without an `advice` field, writes no advice row and calls
//! nobody.
use crate::drafts::{Level, Shape};
use crate::inputs::{FIRST, Planned, route};
use crate::support::{Jev, Stand, command, configure};
use uuid::Uuid;

fn settled(name: &str) -> (&'static str, f64) {
    match name {
        "work" => ("small", 0.7),
        "placement" => ("covered", 0.7),
        "nearest" => ("c1", 0.7),
        // The `model` point asks about the same input; this suite is about `intake`.
        "practical" | "specialist" => ("none", 0.7),
        _ => ("in_line", 0.7),
    }
}

async fn a_disabled_jev_answers_as_main_does(level: Level) {
    let stand = Stand::start("2000").await;
    stand.jev.answer(Jev::Answers(settled));
    let (mut client, repo) = stand.workspace().await;
    let planned = Planned::drafted(&mut client, &repo, level, &Shape::of(2, 2)).await;
    let params = planned.input(FIRST, Uuid::new_v4());
    let route = route(level);

    // No setting row: the state of every workspace on `main`.
    let (plain, plain_bytes) = command(&mut client, route, params.clone()).await;
    assert!(plain.get("advice").is_none(), "{plain}");
    assert_eq!(stand.count("advice_config").await, 0);
    assert_eq!(stand.count("advice_journal").await, 0);
    assert_eq!(planned.recorded(&stand).await.last().unwrap(), FIRST);

    // A setting that says disabled: the replay is the same reply, byte for byte.
    configure(&mut client, false, 0).await;
    let (disabled, disabled_bytes) = command(&mut client, route, params.clone()).await;
    assert_eq!(disabled, plain);
    assert_eq!(disabled_bytes, plain_bytes);
    assert_eq!(stand.count("advice_journal").await, 0);
    assert_eq!(stand.jev.calls(), 0);

    // Enabled: the same reply with the field of each of the two points more, and a
    // journal row for each.
    configure(&mut client, true, 1).await;
    let (enabled, _) = command(&mut client, route, params.clone()).await;
    assert_eq!(enabled["advice"]["status"], "advised");
    let mut without = enabled.clone();
    without.as_object_mut().unwrap().remove("advice");
    without.as_object_mut().unwrap().remove("model_advice");
    assert_eq!(without, plain);
    assert_eq!(stand.jev.asked("fit").len(), 1);
    assert_eq!(stand.rows_of("intake").await, (1, 1));
    assert_eq!(stand.jev.calls(), 2);
    assert_eq!(stand.count("advice_journal").await, 2);

    // Disabled again: the stored fields are not shown and nothing more is written.
    configure(&mut client, false, 2).await;
    let (again, again_bytes) = command(&mut client, route, params).await;
    assert_eq!(again, plain);
    assert_eq!(again_bytes, plain_bytes);
    assert_eq!(stand.jev.calls(), 2);
    assert_eq!(stand.count("advice_journal").await, 2);
    let inputs = planned.recorded(&stand).await;
    assert_eq!(inputs.iter().filter(|text| *text == FIRST).count(), 1);

    // A first input, not a replay, in a workspace whose setting says disabled.
    let (mut other, repo) = stand.workspace().await;
    let fresh = Planned::drafted(&mut other, &repo, level, &Shape::of(2, 2)).await;
    configure(&mut other, false, 0).await;
    let params = fresh.input(FIRST, Uuid::new_v4());
    let (recorded, _) = command(&mut other, route, params).await;
    assert!(recorded.get("advice").is_none(), "{recorded}");
    assert_eq!(
        recorded.as_object().unwrap().keys().collect::<Vec<_>>(),
        plain.as_object().unwrap().keys().collect::<Vec<_>>()
    );
    assert_eq!(stand.jev.calls(), 2);
    assert_eq!(stand.count("advice_journal").await, 2);
    assert_eq!(fresh.recorded(&stand).await.last().unwrap(), FIRST);
    client.finish().await;
    other.finish().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn scope_input_with_jev_disabled_answers_as_main_does() {
    a_disabled_jev_answers_as_main_does(Level::Scopes).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn slice_input_with_jev_disabled_answers_as_main_does() {
    a_disabled_jev_answers_as_main_does(Level::Slices).await;
}
