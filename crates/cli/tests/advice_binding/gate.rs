//! A review that says ready is refused while Jev's advice on a candidate of its draft
//! asks for a change, and passes once the candidate is changed and Jev asks for
//! nothing more.
use crate::binding::{DEBUG, Head, RESEARCH, answers, content, first_split, stored};
use crate::drafts::Level;
use crate::recovery_support::Mcp;
use crate::support::{Jev, Stand, command, configure};
use crate::walk::refused;
use serde_json::{Value, json};

/// The refusal of a ready review over open advice: its code, the rule of its level,
/// where it points, the items it names and the two ways out its intro gives.
pub fn assert_not_followed(refusal: &(Value, String), level: Level, named: &[String]) {
    let (error, intro) = refusal;
    assert_eq!(error["code"], "ADVICE_NOT_FOLLOWED", "{error}");
    assert_eq!(error["refusal"]["code"], "ADVICE_NOT_FOLLOWED");
    let rule = match level {
        Level::Scopes => "SCOPE-REVIEW-ADVICE-OPEN",
        Level::Slices => "SLICE-REVIEW-ADVICE-OPEN",
    };
    assert_eq!(error["refusal"]["rule"], rule, "{error}");
    assert_eq!(error["refusal"]["path"], "/params/review/advice_departures");
    assert_eq!(error["refusal"]["next_action"], level.save_route());
    assert_eq!(
        error["refusal"]["actual"],
        json!(format!(
            "open advice on {} item{}: {}",
            named.len(),
            if named.len() == 1 { "" } else { "s" },
            named.join("; ")
        )),
        "{error}"
    );
    for way in [
        "change it as the advice says and save the draft again",
        level.save_route(),
        "add an entry to advice_departures of this review",
    ] {
        assert!(intro.contains(way), "{way}: {intro}");
    }
    assert!(intro.ends_with("Nothing was stored."), "{intro}");
}

/// One open item as a refusal names it.
pub fn item(candidate: &Value, point: &str, chose: &str) -> String {
    format!(
        "candidate {} {point}: {chose}",
        candidate["id"].as_str().unwrap()
    )
}

/// The head a refused review left: the one it was sent to, with nothing stored.
async fn unchanged(stand: &Stand, client: &mut Mcp, head: &Head, before: (i64, i64)) {
    assert_eq!(stored(stand, head).await, before);
    let again = head.candidates(client).await;
    assert!(!again.is_empty());
    let kept = answers(stand, head.set()).await;
    assert!(
        kept.iter().all(|row| row["departed_at"].is_null()),
        "{kept:?}"
    );
}

async fn a_ready_review_waits_for_the_advice_to_be_followed(level: Level) {
    let stand = Stand::start("2000").await;
    let (mut client, repo) = stand.workspace().await;
    let head = Head::begun(&mut client, &repo, level).await;
    configure(&mut client, true, 0).await;
    stand.jev.answer(Jev::Answers(first_split));

    // Jev wants the first of two new candidates split.
    let (saved, _) = command(&mut client, head.route(), head.first(2)).await;
    assert_eq!(
        saved["advice"]["candidates"][0]["advice"], "split",
        "{saved}"
    );
    let head = head.after(&mut client, &saved).await;
    let candidates = head.candidates(&mut client).await;
    let before = stored(&stand, &head).await;
    assert_eq!(before.0, 0);

    // A ready review is refused: it names the candidate, the point and what Jev chose.
    let review = head.review(&candidates, "ready", None);
    let refusal = refused(&mut client, head.route(), review.clone()).await;
    assert_not_followed(&refusal, level, &[item(&candidates[0], "sizing", "split")]);
    unchanged(&stand, &mut client, &head, before).await;
    // The same review again is refused the same way: nothing was stored of the first.
    let again = refused(&mut client, head.route(), review).await;
    assert_eq!(again.0["refusal"], refusal.0["refusal"]);
    unchanged(&stand, &mut client, &head, before).await;

    // The candidate is changed and saved, and Jev still wants it split: the earlier
    // answer no longer counts, the new one does, and the review is refused again.
    let params = head.again(&mut client, &[1]).await;
    let (saved, _) = command(&mut client, head.route(), params).await;
    assert_eq!(
        saved["advice"]["candidates"][0]["advice"], "split",
        "{saved}"
    );
    let head = head.after(&mut client, &saved).await;
    let candidates = head.candidates(&mut client).await;
    assert_eq!(candidates[0]["revision"], 2);
    let kept = answers(&stand, head.set()).await;
    let about_first = kept
        .iter()
        .filter(|row| row["point"] == "sizing" && row["candidate_id"] == candidates[0]["id"])
        .map(|row| {
            (
                row["candidate_revision"].clone(),
                row["asks_change"].clone(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        about_first,
        [(json!(1), json!(true)), (json!(2), json!(true))]
    );
    let before = stored(&stand, &head).await;
    let review = head.review(&candidates, "ready", None);
    let refusal = refused(&mut client, head.route(), review).await;
    assert_not_followed(&refusal, level, &[item(&candidates[0], "sizing", "split")]);
    unchanged(&stand, &mut client, &head, before).await;

    // Changed once more, and this time Jev asks for nothing: the review passes.
    stand.jev.answer(Jev::Answers(content));
    let params = head.again(&mut client, &[1]).await;
    let (saved, _) = command(&mut client, head.route(), params).await;
    assert_eq!(
        saved["advice"]["candidates"][0]["advice"], "keep",
        "{saved}"
    );
    let head = head.after(&mut client, &saved).await;
    let candidates = head.candidates(&mut client).await;
    assert_eq!(candidates[0]["revision"], 3);
    let review = head.review(&candidates, "ready", None);
    let (reviewed, _) = command(&mut client, head.route(), review).await;
    assert_eq!(reviewed["candidate_set"]["status"], "ready", "{reviewed}");
    assert_eq!(stored(&stand, &head).await.0, 1);
    // The two answers that asked for a change stay as they were given: nobody
    // departed from them, they were followed.
    let kept = answers(&stand, head.set()).await;
    assert_eq!(
        kept.iter().filter(|row| row["asks_change"] == true).count(),
        2
    );
    assert!(kept.iter().all(|row| row["departed_at"].is_null()));
    client.finish().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn a_ready_scope_review_waits_for_the_sizing_advice_to_be_followed() {
    a_ready_review_waits_for_the_advice_to_be_followed(Level::Scopes).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn a_ready_slice_review_waits_for_the_sizing_advice_to_be_followed() {
    a_ready_review_waits_for_the_advice_to_be_followed(Level::Slices).await;
}

/// `sizing` asks for nothing; `pipeline` chooses Research for the second work node,
/// whose draft chose Debug, and agrees on the others.
fn second_researched(name: &str) -> (&'static str, f64) {
    match name {
        "c2.pipeline" => (RESEARCH, 0.8),
        other => content(other),
    }
}

/// Jev wants the second work node split and its pipeline to be Research.
fn second_both(name: &str) -> (&'static str, f64) {
    match name {
        "c2.advice" => ("split", 0.8),
        other => second_researched(other),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn a_ready_slice_review_waits_for_the_pipeline_advice_to_be_followed() {
    let level = Level::Slices;
    let stand = Stand::start("2000").await;
    let (mut client, repo) = stand.workspace().await;
    let head = Head::begun(&mut client, &repo, level).await;
    configure(&mut client, true, 0).await;
    stand.jev.answer(Jev::Answers(second_both));

    let (saved, _) = command(&mut client, head.route(), head.first(2)).await;
    assert_eq!(
        saved["pipeline_advice"]["candidates"][1]["differs"], true,
        "{saved}"
    );
    let head = head.after(&mut client, &saved).await;
    let candidates = head.candidates(&mut client).await;
    assert_eq!(candidates[1]["pipeline"], DEBUG);
    let before = stored(&stand, &head).await;

    // Both points are open on the second node: the refusal names each.
    let review = head.review(&candidates, "ready", None);
    let refusal = refused(&mut client, head.route(), review).await;
    let second = &candidates[1];
    assert_not_followed(
        &refusal,
        level,
        &[
            item(second, "sizing", "split"),
            item(second, "pipeline", RESEARCH),
        ],
    );
    unchanged(&stand, &mut client, &head, before).await;

    // The node is changed and saved; Jev no longer wants it split and still chooses
    // another pipeline: `pipeline` alone is open now.
    stand.jev.answer(Jev::Answers(second_researched));
    let params = head.again(&mut client, &[2]).await;
    let (saved, _) = command(&mut client, head.route(), params).await;
    let head = head.after(&mut client, &saved).await;
    let candidates = head.candidates(&mut client).await;
    let before = stored(&stand, &head).await;
    let review = head.review(&candidates, "ready", None);
    let refusal = refused(&mut client, head.route(), review).await;
    assert_not_followed(
        &refusal,
        level,
        &[item(&candidates[1], "pipeline", RESEARCH)],
    );
    unchanged(&stand, &mut client, &head, before).await;

    // The draft takes the pipeline Jev chose: Jev agrees, and the review passes.
    let mut params = head.again(&mut client, &[2]).await;
    params["draft"]["nodes"][1]["pipeline"] = json!(RESEARCH);
    params["draft"]["nodes"][1]["pipeline_reason"] = json!("The cause needs research first");
    let (saved, _) = command(&mut client, head.route(), params).await;
    assert_eq!(
        saved["pipeline_advice"]["candidates"],
        json!([{"candidate":2,"pipeline":RESEARCH,"pipeline_p":0.8,"draft_pipeline_p":0.8,
            "differs":false}]),
        "{saved}"
    );
    let head = head.after(&mut client, &saved).await;
    let candidates = head.candidates(&mut client).await;
    let review = head.review(&candidates, "ready", None);
    let (reviewed, _) = command(&mut client, head.route(), review).await;
    assert_eq!(reviewed["candidate_set"]["status"], "ready", "{reviewed}");
    client.finish().await;
}

/// Jev wants the second work node moved into a Scope of its own.
fn second_elsewhere(name: &str) -> (&'static str, f64) {
    match name {
        "c2.advice" => ("separate_scope", 0.8),
        other => content(other),
    }
}

/// What `main` leaves of "follow the advice" for `separate_scope`: a Slice draft may
/// not drop a node without a replacement or a result, so the advised node cannot
/// simply leave this Scope's plan. The advice is closed by a departure, or by any
/// saved change of the node that Jev then asks nothing about.
#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn a_node_advised_into_a_scope_of_its_own_cannot_be_dropped_from_the_draft() {
    let level = Level::Slices;
    let stand = Stand::start("2000").await;
    let (mut client, repo) = stand.workspace().await;
    let head = Head::begun(&mut client, &repo, level).await;
    configure(&mut client, true, 0).await;
    stand.jev.answer(Jev::Answers(second_elsewhere));
    let (saved, _) = command(&mut client, head.route(), head.first(2)).await;
    assert_eq!(
        saved["advice"]["candidates"][1]["advice"], "separate_scope",
        "{saved}"
    );
    let head = head.after(&mut client, &saved).await;
    let candidates = head.candidates(&mut client).await;
    let second = &candidates[1];
    let review = head.review(&candidates, "ready", None);
    let refusal = refused(&mut client, head.route(), review).await;
    assert_not_followed(&refusal, level, &[item(second, "sizing", "separate_scope")]);

    // The draft without the advised node: refused, with and without a supersession
    // that names no replacement and no result.
    let mut dropped = head.again(&mut client, &[]).await;
    dropped["draft"]["nodes"].as_array_mut().unwrap().remove(1);
    let (error, _) = refused(&mut client, head.route(), dropped.clone()).await;
    assert_eq!(error["code"], "invalid_arguments", "{error}");
    dropped["draft"]["supersessions"] = json!([{"candidate_id":second["id"],
        "revision":second["revision"],"reason":"It belongs in a Scope of its own"}]);
    let (error, _) = refused(&mut client, head.route(), dropped).await;
    assert_eq!(error["code"], "invalid_arguments", "{error}");

    // The review passes with the departure that says so.
    let stated = json!([crate::binding::departure(
        second,
        "sizing",
        "It cannot leave this plan without a replacement; planned as a separate Scope next"
    )]);
    let review = head.review(&candidates, "ready", Some(stated));
    let (reviewed, _) = command(&mut client, head.route(), review).await;
    assert_eq!(reviewed["candidate_set"]["status"], "ready", "{reviewed}");
    client.finish().await;
}
