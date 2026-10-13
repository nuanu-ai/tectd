//! Disabled means the reply without the feature: with no setting, or with a setting
//! that says disabled, a recorded input answers with neither field, writes no advice
//! row and calls nobody.
use crate::drafts::{Level, Shape};
use crate::inputs::{FIRST, Planned, route};
use crate::model::{advised, intake_advised, settled};
use crate::support::{Jev, Stand, command, configure};
use uuid::Uuid;

async fn a_disabled_jev_adds_no_field_and_writes_no_row(level: Level) {
    let stand = Stand::start("2000").await;
    stand.jev.answer(Jev::Answers(settled));
    let (mut client, repo) = stand.workspace().await;
    let planned = Planned::drafted(&mut client, &repo, level, &Shape::of(2, 2)).await;
    let params = planned.input(FIRST, Uuid::new_v4());
    let route = route(level);

    // No setting row, then a setting that says disabled: the same reply, byte for
    // byte, with neither field, and nothing written or asked.
    let (plain, plain_bytes) = command(&mut client, route, params.clone()).await;
    assert!(plain.get("advice").is_none() && plain.get("model_advice").is_none());
    assert_eq!(stand.count("advice_config").await, 0);
    assert_eq!(planned.recorded(&stand).await.last().unwrap(), FIRST);
    configure(&mut client, false, 0).await;
    let (disabled, disabled_bytes) = command(&mut client, route, params.clone()).await;
    assert_eq!(disabled, plain);
    assert_eq!(disabled.to_string(), plain.to_string());
    assert_eq!(disabled_bytes, plain_bytes);
    assert_eq!(stand.count("advice_journal").await, 0);
    assert_eq!(stand.jev.calls(), 0);

    // Enabled: the same reply with exactly the two fields more.
    configure(&mut client, true, 1).await;
    let (enabled, _) = command(&mut client, route, params.clone()).await;
    let mut without = enabled.clone();
    let fields = without.as_object_mut().unwrap();
    assert_eq!(
        fields.remove("advice").unwrap(),
        intake_advised(level, true)
    );
    assert_eq!(fields.remove("model_advice").unwrap(), advised());
    assert_eq!(without, plain);
    assert_eq!(without.to_string(), plain.to_string());
    assert_eq!(
        (stand.jev.calls(), stand.count("advice_journal").await),
        (2, 2)
    );

    // Disabled again: neither stored field is shown and nothing more is written.
    configure(&mut client, false, 2).await;
    let (again, again_bytes) = command(&mut client, route, params).await;
    assert_eq!(again.to_string(), plain.to_string());
    assert_eq!(again_bytes, plain_bytes);
    assert_eq!(
        (stand.jev.calls(), stand.count("advice_journal").await),
        (2, 2)
    );
    let inputs = planned.recorded(&stand).await;
    assert_eq!(inputs.iter().filter(|text| *text == FIRST).count(), 1);

    // A first input, not a replay, in a workspace whose setting says disabled.
    let (mut other, repo) = stand.workspace().await;
    let fresh = Planned::drafted(&mut other, &repo, level, &Shape::of(2, 2)).await;
    configure(&mut other, false, 0).await;
    let params = fresh.input(FIRST, Uuid::new_v4());
    let (recorded, recorded_bytes) = command(&mut other, route, params).await;
    assert!(recorded.get("advice").is_none() && recorded.get("model_advice").is_none());
    assert_eq!(
        recorded.as_object().unwrap().keys().collect::<Vec<_>>(),
        plain.as_object().unwrap().keys().collect::<Vec<_>>()
    );
    // The two sets differ in their ids and in nothing that has a length.
    assert_eq!(recorded_bytes, plain_bytes);
    assert_eq!(
        (stand.jev.calls(), stand.count("advice_journal").await),
        (2, 2)
    );
    assert_eq!(fresh.recorded(&stand).await.last().unwrap(), FIRST);
    client.finish().await;
    other.finish().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn scope_input_with_jev_disabled_carries_neither_field() {
    a_disabled_jev_adds_no_field_and_writes_no_row(Level::Scopes).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn slice_input_with_jev_disabled_carries_neither_field() {
    a_disabled_jev_adds_no_field_and_writes_no_row(Level::Slices).await;
}
