//! What leaves nothing open: a changed candidate Jev gave no advice on, a disabled
//! Jev, and a replayed command.
use crate::binding::{Head, abstaining, answers, departure, first_split, stored};
use crate::drafts::Level;
use crate::gate::{assert_not_followed, item};
use crate::support::{Jev, Stand, command, configure};
use crate::walk::refused;
use serde_json::json;

async fn no_advice_on_the_changed_candidate_leaves_nothing_open(level: Level) {
    let stand = Stand::start("2000").await;
    // Jev answers with something that is not an answer, or abstains on everything.
    for failing in [Jev::Garbage, Jev::Answers(abstaining)] {
        let (mut client, repo) = stand.workspace().await;
        let head = Head::begun(&mut client, &repo, level).await;
        configure(&mut client, true, 0).await;
        stand.jev.answer(Jev::Answers(first_split));
        let (saved, _) = command(&mut client, head.route(), head.first(2)).await;
        let head = head.after(&mut client, &saved).await;
        let candidates = head.candidates(&mut client).await;
        let review = head.review(&candidates, "ready", None);
        let refusal = refused(&mut client, head.route(), review).await;
        assert_not_followed(&refusal, level, &[item(&candidates[0], "sizing", "split")]);

        // The candidate is changed and saved, and this time Jev gives no advice: the
        // draft is stored, nothing is kept, and the earlier answer no longer counts.
        stand.jev.answer(failing);
        let kept = answers(&stand, head.set()).await;
        let params = head.again(&mut client, &[1]).await;
        let (saved, _) = command(&mut client, head.route(), params).await;
        assert_ne!(saved["advice"]["status"], "advised", "{saved}");
        let head = head.after(&mut client, &saved).await;
        assert_eq!(answers(&stand, head.set()).await, kept);
        let candidates = head.candidates(&mut client).await;
        assert_eq!(candidates[0]["revision"], 2);
        let review = head.review(&candidates, "ready", None);
        let (reviewed, _) = command(&mut client, head.route(), review).await;
        assert_eq!(reviewed["candidate_set"]["status"], "ready", "{reviewed}");
        client.finish().await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn no_advice_on_the_changed_scope_candidate_leaves_nothing_open() {
    no_advice_on_the_changed_candidate_leaves_nothing_open(Level::Scopes).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn no_advice_on_the_changed_slice_work_node_leaves_nothing_open() {
    no_advice_on_the_changed_candidate_leaves_nothing_open(Level::Slices).await;
}

async fn a_disabled_jev_holds_no_review_and_writes_nothing(level: Level) {
    let stand = Stand::start("2000").await;
    let (mut client, repo) = stand.workspace().await;
    let head = Head::begun(&mut client, &repo, level).await;
    configure(&mut client, true, 0).await;
    stand.jev.answer(Jev::Answers(first_split));
    let (saved, _) = command(&mut client, head.route(), head.first(2)).await;
    let head = head.after(&mut client, &saved).await;
    let candidates = head.candidates(&mut client).await;
    let kept = answers(&stand, head.set()).await;
    assert_eq!(kept[0]["asks_change"], true);

    // Jev is switched off with that advice open. The review says ready and even
    // states a departure: it passes as on `main`, and no advice table is written.
    configure(&mut client, false, 1).await;
    let before = (
        stand.jev.calls(),
        stand.count("advice_journal").await,
        stand.count("advice_candidate_answers").await,
    );
    let stated = json!([departure(&candidates[0], "sizing", "It is one outcome")]);
    let review = head.review(&candidates, "ready", Some(stated));
    let (reviewed, _) = command(&mut client, head.route(), review).await;
    assert_eq!(reviewed["candidate_set"]["status"], "ready", "{reviewed}");
    assert_eq!(stored(&stand, &head).await.0, 1);
    assert_eq!(answers(&stand, head.set()).await, kept);
    assert_eq!(
        (
            stand.jev.calls(),
            stand.count("advice_journal").await,
            stand.count("advice_candidate_answers").await,
        ),
        before
    );
    client.finish().await;

    // The same with nothing stated, in a workspace of its own.
    let (mut other, repo) = stand.workspace().await;
    let head = Head::begun(&mut other, &repo, level).await;
    configure(&mut other, true, 0).await;
    let (saved, _) = command(&mut other, head.route(), head.first(2)).await;
    let head = head.after(&mut other, &saved).await;
    let candidates = head.candidates(&mut other).await;
    configure(&mut other, false, 1).await;
    let review = head.review(&candidates, "ready", None);
    let (reviewed, _) = command(&mut other, head.route(), review).await;
    assert_eq!(reviewed["candidate_set"]["status"], "ready", "{reviewed}");
    let kept = answers(&stand, head.set()).await;
    assert!(kept.iter().all(|row| row["departed_at"].is_null()));
    other.finish().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn a_disabled_jev_holds_no_scope_review_and_writes_nothing() {
    a_disabled_jev_holds_no_review_and_writes_nothing(Level::Scopes).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn a_disabled_jev_holds_no_slice_review_and_writes_nothing() {
    a_disabled_jev_holds_no_review_and_writes_nothing(Level::Slices).await;
}

async fn a_replayed_save_and_a_replayed_review_write_nothing_twice(level: Level) {
    let stand = Stand::start("2000").await;
    let (mut client, repo) = stand.workspace().await;
    let head = Head::begun(&mut client, &repo, level).await;
    configure(&mut client, true, 0).await;
    stand.jev.answer(Jev::Answers(first_split));
    let draft = head.first(2);
    let (saved, _) = command(&mut client, head.route(), draft.clone()).await;
    let head = head.after(&mut client, &saved).await;
    let candidates = head.candidates(&mut client).await;

    let stated = json!([departure(&candidates[0], "sizing", "It is one outcome")]);
    let review = head.review(&candidates, "ready", Some(stated));
    let (reviewed, _) = command(&mut client, head.route(), review.clone()).await;
    assert_eq!(reviewed["candidate_set"]["status"], "ready", "{reviewed}");
    let kept = answers(&stand, head.set()).await;
    assert_eq!(kept[0]["departure_reason"], "It is one outcome");
    let before = (
        stand.jev.calls(),
        stand.count("advice_journal").await,
        stand.count("advice_candidate_answers").await,
        stored(&stand, &head).await,
    );

    // The review again, and the draft save again, each under its own request id:
    // the same replies, the same rows, the same time of departure.
    for _ in 0..2 {
        let (replayed, _) = command(&mut client, head.route(), review.clone()).await;
        assert_eq!(replayed, reviewed);
        let (replayed, _) = command(&mut client, head.route(), draft.clone()).await;
        assert_eq!(replayed, saved);
    }
    assert_eq!(answers(&stand, head.set()).await, kept);
    assert_eq!(
        (
            stand.jev.calls(),
            stand.count("advice_journal").await,
            stand.count("advice_candidate_answers").await,
            stored(&stand, &head).await,
        ),
        before
    );
    client.finish().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn a_replayed_scope_save_and_review_write_nothing_twice() {
    a_replayed_save_and_a_replayed_review_write_nothing_twice(Level::Scopes).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn a_replayed_slice_save_and_review_write_nothing_twice() {
    a_replayed_save_and_a_replayed_review_write_nothing_twice(Level::Slices).await;
}
