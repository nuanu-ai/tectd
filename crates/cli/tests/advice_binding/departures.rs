//! The departures a review states: what is taken for one, what a departure does
//! where no advice is open, and how a ready review passes with one for each open item.
use crate::binding::{Head, answers, content, departure, first_split, stored, two_split};
use crate::drafts::Level;
use crate::gate::{assert_not_followed, item};
use crate::support::{Jev, Stand, command, configure};
use crate::walk::refused;
use serde_json::{Value, json};

/// The reason and whether a time stands on each kept `sizing` answer, in draft order.
fn departed(kept: &[Value]) -> Vec<(Value, bool)> {
    kept.iter()
        .filter(|row| row["point"] == "sizing")
        .map(|row| {
            (
                row["departure_reason"].clone(),
                row["departed_at"].is_string(),
            )
        })
        .collect()
}

async fn a_ready_review_passes_with_a_departure_for_each_open_item(level: Level) {
    let stand = Stand::start("2000").await;
    let (mut client, repo) = stand.workspace().await;
    let head = Head::begun(&mut client, &repo, level).await;
    configure(&mut client, true, 0).await;
    // Jev wants the first two of three new candidates split.
    stand.jev.answer(Jev::Answers(two_split));
    let (saved, _) = command(&mut client, head.route(), head.first(3)).await;
    let head = head.after(&mut client, &saved).await;
    let candidates = head.candidates(&mut client).await;
    let nothing = vec![(Value::Null, false); 3];
    assert_eq!(departed(&answers(&stand, head.set()).await), nothing);

    // A departure for one of the two open items: refused, and the refusal names the
    // other one alone. The stated departure is not written: nothing was stored.
    let first = departure(
        &candidates[0],
        "sizing",
        "It is one outcome and cannot be split",
    );
    let params = head.review(&candidates, "ready", Some(json!([first])));
    let refusal = refused(&mut client, head.route(), params).await;
    assert_not_followed(&refusal, level, &[item(&candidates[1], "sizing", "split")]);
    assert_eq!(stored(&stand, &head).await.0, 0);
    assert_eq!(departed(&answers(&stand, head.set()).await), nothing);

    // A review that says revise passes with both items open and writes nothing on them.
    let params = head.review(&candidates, "revise", None);
    let (revised, _) = command(&mut client, head.route(), params).await;
    assert_eq!(revised["candidate_set"]["status"], "review_required");
    assert_eq!(stored(&stand, &head).await.0, 1);
    assert_eq!(departed(&answers(&stand, head.set()).await), nothing);
    let head = head.after(&mut client, &revised).await;

    // A departure for each open item: the review passes, and the reason and the time
    // stand on the two answers it departs from and on no other.
    let second = departure(&candidates[1], "sizing", "The user asked for exactly this");
    let params = head.review(&candidates, "ready", Some(json!([first, second])));
    let (reviewed, _) = command(&mut client, head.route(), params).await;
    assert_eq!(reviewed["candidate_set"]["status"], "ready", "{reviewed}");
    let kept = answers(&stand, head.set()).await;
    assert_eq!(
        departed(&kept),
        [
            (json!("It is one outcome and cannot be split"), true),
            (json!("The user asked for exactly this"), true),
            (Value::Null, false)
        ]
    );
    // The time is the review's, after the answer was given; the answer itself stays.
    let later: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM advice_candidate_answers \
         WHERE tenant_id=$1 AND departed_at > created_at",
    )
    .bind(stand.tenant)
    .fetch_one(&stand.pool)
    .await
    .unwrap();
    assert_eq!(later, 2);
    assert_eq!(kept[0]["answer"], saved["advice"]["candidates"][0]);
    assert_eq!(kept[0]["asks_change"], true);
    client.finish().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn a_ready_scope_review_passes_with_a_departure_for_each_open_item() {
    a_ready_review_passes_with_a_departure_for_each_open_item(Level::Scopes).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn a_ready_slice_review_passes_with_a_departure_for_each_open_item() {
    a_ready_review_passes_with_a_departure_for_each_open_item(Level::Slices).await;
}

async fn a_departure_stated_by_a_review_that_says_revise_closes_the_advice(level: Level) {
    let stand = Stand::start("2000").await;
    let (mut client, repo) = stand.workspace().await;
    let head = Head::begun(&mut client, &repo, level).await;
    configure(&mut client, true, 0).await;
    stand.jev.answer(Jev::Answers(first_split));
    let (saved, _) = command(&mut client, head.route(), head.first(2)).await;
    let head = head.after(&mut client, &saved).await;
    let candidates = head.candidates(&mut client).await;

    let stated = departure(&candidates[0], "sizing", "It is one outcome");
    let params = head.review(&candidates, "revise", Some(json!([stated])));
    let (revised, _) = command(&mut client, head.route(), params).await;
    assert_eq!(revised["candidate_set"]["status"], "review_required");
    assert_eq!(
        departed(&answers(&stand, head.set()).await),
        [(json!("It is one outcome"), true), (Value::Null, false)]
    );
    // Nothing is open any more: the next review says ready and states nothing.
    let head = head.after(&mut client, &revised).await;
    let params = head.review(&candidates, "ready", None);
    let (reviewed, _) = command(&mut client, head.route(), params).await;
    assert_eq!(reviewed["candidate_set"]["status"], "ready", "{reviewed}");
    client.finish().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn a_departure_stated_by_a_scope_review_that_says_revise_closes_the_advice() {
    a_departure_stated_by_a_review_that_says_revise_closes_the_advice(Level::Scopes).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn a_departure_stated_by_a_slice_review_that_says_revise_closes_the_advice() {
    a_departure_stated_by_a_review_that_says_revise_closes_the_advice(Level::Slices).await;
}

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
