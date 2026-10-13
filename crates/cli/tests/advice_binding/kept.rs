//! What a draft save keeps of Jev's advice: one row per asked candidate and point,
//! written with the journal row of an advised call and never twice.
use crate::binding::{Head, abstaining, answers, first_split};
use crate::drafts::Level;
use crate::support::{Jev, Stand, command, configure};
use serde_json::{Value, json};

async fn an_advised_save_keeps_one_answer_per_asked_candidate(level: Level) {
    let stand = Stand::start("2000").await;
    let (mut client, repo) = stand.workspace().await;
    let head = Head::begun(&mut client, &repo, level).await;
    configure(&mut client, true, 0).await;
    stand.jev.answer(Jev::Answers(first_split));

    let params = head.first(2);
    let (saved, _) = command(&mut client, head.route(), params.clone()).await;
    assert_eq!(
        saved["advice"]["candidates"][0]["advice"], "split",
        "{saved}"
    );
    let head = head.after(&mut client, &saved).await;
    let candidates = head.candidates(&mut client).await;
    let rows = answers(&stand, head.set()).await;

    // One row per asked candidate of `sizing`, and on the Slice level one more per
    // asked work node of `pipeline`, in the order the points fired.
    let mut expected = vec![("sizing", &saved["advice"], [true, false])];
    if level == Level::Slices {
        expected.push(("pipeline", &saved["pipeline_advice"], [false, false]));
    }
    assert_eq!(rows.len(), expected.len() * 2, "{rows:?}");
    for (index, (point, field, asks)) in expected.into_iter().enumerate() {
        let journal = stand.newest_row_of(point).await;
        for position in 0..2 {
            let row = &rows[index * 2 + position];
            assert_eq!(row["point"], point, "{row}");
            assert_eq!(row["level"], level.name(), "{row}");
            assert_eq!(row["journal_id"], journal["id"], "{row}");
            assert_eq!(row["workspace_id"], journal["workspace_id"], "{row}");
            assert_eq!(row["candidate_set_id"], head.set()["id"], "{row}");
            assert_eq!(row["candidate_id"], candidates[position]["id"], "{row}");
            assert_eq!(row["candidate_revision"], 1, "{row}");
            assert_eq!(row["asks_change"], asks[position], "{row}");
            // The answer is the candidate's entry of the field the agent received.
            assert_eq!(row["answer"], field["candidates"][position], "{row}");
            assert_eq!(
                (&row["departure_reason"], &row["departed_at"]),
                (&Value::Null, &Value::Null)
            );
        }
    }
    // Tokens and probabilities only: no text of the draft is kept.
    let text = json!(rows).to_string();
    for absent in ["The cause is demonstrated", "item 1", "Scope 1", "Work 1"] {
        assert!(!text.contains(absent), "{absent}");
    }

    // A replay of the save returns the same reply and writes nothing twice.
    let before = (
        stand.jev.calls(),
        stand.count("advice_journal").await,
        stand.count("advice_candidate_answers").await,
    );
    for _ in 0..2 {
        let (replayed, _) = command(&mut client, head.route(), params.clone()).await;
        assert_eq!(replayed, saved);
    }
    assert_eq!(
        (
            stand.jev.calls(),
            stand.count("advice_journal").await,
            stand.count("advice_candidate_answers").await,
        ),
        before
    );
    assert_eq!(answers(&stand, head.set()).await, rows);

    // Jev abstains on everything, or answers with something that is not an answer:
    // the journal has its row and nothing is kept.
    for failing in [Jev::Answers(abstaining), Jev::Garbage] {
        let (mut other, repo) = stand.workspace().await;
        let head = Head::begun(&mut other, &repo, level).await;
        configure(&mut other, true, 0).await;
        stand.jev.answer(failing);
        let journalled = stand.count("advice_journal").await;
        let (saved, _) = command(&mut other, head.route(), head.first(2)).await;
        assert_ne!(saved["advice"]["status"], "advised", "{saved}");
        assert!(stand.count("advice_journal").await > journalled);
        let head = head.after(&mut other, &saved).await;
        assert!(answers(&stand, head.set()).await.is_empty());
        other.finish().await;
    }
    assert_eq!(stand.count("advice_candidate_answers").await, before.2);
    client.finish().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn a_scope_draft_save_keeps_jevs_answer_about_each_asked_candidate() {
    an_advised_save_keeps_one_answer_per_asked_candidate(Level::Scopes).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn a_slice_draft_save_keeps_jevs_answer_about_each_asked_work_node() {
    an_advised_save_keeps_one_answer_per_asked_candidate(Level::Slices).await;
}
