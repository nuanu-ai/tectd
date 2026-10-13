//! The departures a review states: what is taken for one, and what a departure does
//! where no advice is open.
use crate::binding::{Head, answers, content, departure, stored};
use crate::drafts::Level;
use crate::support::{Jev, Stand, command, configure};
use crate::walk::refused;
use serde_json::{Value, json};

async fn a_review_takes_a_list_of_departures_and_nothing_else(level: Level) {
    let stand = Stand::start("2000").await;
    let (mut client, repo) = stand.workspace().await;
    let head = Head::begun(&mut client, &repo, level).await;
    configure(&mut client, true, 0).await;
    // Jev asks for nothing: no advice is open on this draft.
    stand.jev.answer(Jev::Answers(content));
    let (saved, _) = command(&mut client, head.route(), head.first(2)).await;
    let head = head.after(&mut client, &saved).await;
    let candidates = head.candidates(&mut client).await;
    let kept = answers(&stand, head.set()).await;
    assert!(kept.iter().all(|row| row["asks_change"] == false));

    // A blank reason, a point no review is held to, one candidate and point named
    // twice, and null where the list goes: each is refused as invalid arguments.
    let first = &candidates[0];
    for (invalid, pointer) in [
        (
            json!([departure(first, "sizing", "   ")]),
            "/params/review/advice_departures/0/reason",
        ),
        (
            json!([
                departure(first, "sizing", "It is one outcome"),
                departure(&candidates[1], "sizing", "It is one outcome"),
                departure(first, "sizing", "Said again")
            ]),
            "/params/review/advice_departures/2",
        ),
        (json!([departure(first, "intake", "It is one outcome")]), ""),
        (Value::Null, ""),
    ] {
        let params = head.review(&candidates, "ready", Some(invalid.clone()));
        let (error, _) = refused(&mut client, head.route(), params).await;
        assert_eq!(error["code"], "invalid_arguments", "{invalid}: {error}");
        if !pointer.is_empty() {
            assert_eq!(error["details"]["pointer"], pointer, "{error}");
        }
    }
    assert_eq!(stored(&stand, &head).await.0, 0);

    // A departure about a candidate without open advice is accepted: the review is
    // stored, and nothing is written on the kept answers.
    let stated = json!([
        departure(&candidates[1], "sizing", "Not what was advised"),
        departure(&candidates[1], "pipeline", "Not what was advised")
    ]);
    let params = head.review(&candidates, "ready", Some(stated));
    let (reviewed, _) = command(&mut client, head.route(), params).await;
    assert_eq!(reviewed["candidate_set"]["status"], "ready", "{reviewed}");
    assert_eq!(stored(&stand, &head).await.0, 1);
    assert_eq!(answers(&stand, head.set()).await, kept);
    client.finish().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn a_scope_review_takes_a_list_of_departures_and_nothing_else() {
    a_review_takes_a_list_of_departures_and_nothing_else(Level::Scopes).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn a_slice_review_takes_a_list_of_departures_and_nothing_else() {
    a_review_takes_a_list_of_departures_and_nothing_else(Level::Slices).await;
}
