//! A draft reads the same to Jev whether a save carries it or the store holds it.
use super::*;

fn decoded<T: serde::de::DeserializeOwned>(value: Value) -> T {
    serde_json::from_value(value).unwrap()
}

#[test]
fn a_stored_scope_draft_reads_as_the_draft_a_save_carried() {
    let (send, accept) = (Uuid::new_v4(), Uuid::new_v4());
    let carried: ScopeCandidateDraft = decoded(json!({"boundary":"finite","goals":[],
    "candidates":[
        {"identity":{"local":"send"},"title":"Send invitations","outcome":"A user is invited",
         "trigger":"t","delivered_behavior":"b","proof":"p",
         "includes":["invitation form","audit dashboard"],"excludes":["billing"],
         "dependencies":[{"id":accept}],"coverage_goals":[]},
        {"identity":{"id":accept,"revision":2},"title":"Accept invitations",
         "outcome":"An invited user joins","trigger":"t","delivered_behavior":"b",
         "proof":"p","coverage_goals":[]}
    ]}));
    let entity = |id: Uuid,
                  title: &str,
                  outcome: &str,
                  includes: Value,
                  excludes: Value,
                  dependencies: Value| {
        json!({"id":id,"revision":2,"title":title,"outcome":outcome,"trigger":"t",
            "delivered_behavior":"b","proof":"p","includes":includes,"excludes":excludes,
            "dependencies":dependencies,"coverage_goal_ids":[],"evidence_ids":[]})
    };
    let stored: ResolvedCandidateDraft = decoded(json!({"boundary":"finite","goals":[],
    "evidence":[],"blockers":[],"protected_changes":[],
    "candidates":[
        entity(send, "Send invitations", "A user is invited",
            json!(["invitation form","audit dashboard"]), json!(["billing"]), json!([accept])),
        entity(accept, "Accept invitations", "An invited user joins", json!([]), json!([]),
            json!([]))
    ]}));
    let view = DraftView::saved_scope_draft(&stored).unwrap();
    assert_eq!(Some(&view), DraftView::scope_draft(&carried).as_ref());
    assert_eq!(
        view.candidates,
        [
            json!({"key":"c1","title":"Send invitations","outcome":"A user is invited",
                "includes":[{"key":"c1.i1","text":"invitation form"},
                    {"key":"c1.i2","text":"audit dashboard"}],
                "excludes":["billing"],"dependencies":["c2"]}),
            json!({"key":"c2","title":"Accept invitations","outcome":"An invited user joins",
                "includes":[],"excludes":[],"dependencies":[]})
        ]
    );
    let asked = |position, items| Asked { position, items };
    assert_eq!(view.asked, [asked(1, 2), asked(2, 0)]);
}

#[test]
fn a_stored_slice_draft_reads_as_the_draft_a_save_carried() {
    let (form, mail, delivery) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
    let carried: SliceCandidateDraft = decoded(json!({"coverage_summary":"Whole Scope","nodes":[
        {"kind":"work","identity":{"candidate_id":form,"revision":1},"title":"Invitation form",
         "outcome":"The form sends an invitation","includes":["form"],"excludes":["styling"],
         "proof":["p"],"pipeline":"slice.lightweight-tdd-development","pipeline_reason":"r"},
        {"kind":"decision","identity":{"local":"mail"},"title":"Choose the mail provider",
         "question":"Which provider sends the mail?","resolution_criteria":["c"],
         "dependencies":[{"candidate_id":form,"revision":1}]},
        {"kind":"work","identity":{"local":"delivery"},"title":"Delivery",
         "outcome":"The invitation arrives","includes":["sending","retries","metrics"],
         "dependencies":[{"local":"mail"}],"proof":["p"],
         "pipeline":"slice.lightweight-tdd-development","pipeline_reason":"r"}
    ]}));
    let work = |id: Uuid,
                title: &str,
                outcome: &str,
                includes: Value,
                excludes: Value,
                dependencies: Value| {
        json!({"kind":"work","id":id,"revision":1,"title":title,"outcome":outcome,
            "includes":includes,"excludes":excludes,"dependencies":dependencies,"proof":["p"],
            "pipeline":"slice.lightweight-tdd-development","pipeline_reason":"r",
            "why_lightweight_insufficient":null,"why_further_vertical_split_not_viable":null,
            "source_result_ids":[]})
    };
    let stored: ResolvedSliceCandidateDraft = decoded(json!({"coverage_summary":"Whole Scope",
    "supersessions":[],"nodes":[
        work(form, "Invitation form", "The form sends an invitation", json!(["form"]),
            json!(["styling"]), json!([])),
        {"kind":"decision","id":mail,"revision":1,"title":"Choose the mail provider",
         "question":"Which provider sends the mail?","resolution_criteria":["c"],
         "dependencies":[form],"source_result_ids":[]},
        work(delivery, "Delivery", "The invitation arrives",
            json!(["sending","retries","metrics"]), json!([]), json!([mail]))
    ]}));
    let view = DraftView::saved_slice_draft(&stored).unwrap();
    assert_eq!(Some(&view), DraftView::slice_draft(&carried).as_ref());
    assert_eq!(
        view.candidates[1],
        json!({"key":"c2","kind":"decision","title":"Choose the mail provider",
            "question":"Which provider sends the mail?","dependencies":["c1"]})
    );
    assert_eq!(view.candidates[2]["kind"], "work");
    assert_eq!(view.candidates[2]["dependencies"], json!(["c2"]));
    // Positions count every node; the decision node is not asked about.
    let asked = |position, items| Asked { position, items };
    assert_eq!(view.asked, [asked(1, 1), asked(3, 3)]);
}

#[test]
fn a_dependency_outside_the_draft_gives_no_view() {
    let stored: ResolvedSliceCandidateDraft = decoded(json!({"coverage_summary":"s",
        "supersessions":[],"nodes":[{"kind":"decision","id":Uuid::new_v4(),"revision":1,
            "title":"t","question":"q","resolution_criteria":["c"],
            "dependencies":[Uuid::new_v4()],"source_result_ids":[]}]}));
    assert_eq!(DraftView::saved_slice_draft(&stored), None);
    let empty: ResolvedSliceCandidateDraft =
        decoded(json!({"coverage_summary":"s","supersessions":[],"nodes":[]}));
    let view = DraftView::saved_slice_draft(&empty).unwrap();
    assert!(view.candidates.is_empty() && view.asked.is_empty());
}
