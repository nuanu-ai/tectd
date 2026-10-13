//! A first draft: every work node is new, so Jev is asked about each, beside `sizing`.
use crate::pipeline::{
    DEBUG, LIGHTWEIGHT, RESEARCH, asked, decision, draft, planning, points, rows, save, work,
};
use crate::support::{Jev, KEY, Stand, Weights, configure};
use serde_json::json;
use tect_domain::PIPELINE_NOTE;

const FIRST_REASON: &str = "REASON-ALPHA the cause is unknown";
const SECOND_REASON: &str = "REASON-BRAVO a bounded change";

/// `sizing` keeps everything; for `pipeline` Jev agrees with the draft on the first
/// work node, and for the second chooses another pipeline and gives the one the draft
/// chose a probability no other criterion has.
fn agreeing_then_differing(name: &str) -> Weights {
    match name {
        "c1.pipeline" => ((DEBUG, 0.74), None),
        "c3.pipeline" => ((RESEARCH, 0.62), Some((LIGHTWEIGHT, 0.21))),
        size if size.ends_with(".size") => (("bounded", 0.7), None),
        advice if advice.ends_with(".advice") => (("keep", 0.7), None),
        other if other.ends_with(".pipeline") => panic!("unexpected question {other}"),
        _ => (("requested", 0.7), None),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn a_first_draft_gets_pipeline_advice_on_each_work_node_beside_sizing() {
    let stand = Stand::start("2000").await;
    let (mut client, repo) = stand.workspace().await;
    let (head, program_id) = planning(&mut client, &repo).await;
    configure(&mut client, true, 0).await;
    stand.jev.answer(Jev::Weighted(agreeing_then_differing));

    // Two work nodes with a decision node between them: draft positions 1, 2 and 3.
    let nodes = vec![
        work(1, DEBUG, FIRST_REASON),
        decision("decision", "Is Lightweight sufficient?"),
        work(2, LIGHTWEIGHT, SECOND_REASON),
    ];
    let (saved, next, request_id) = save(&mut client, &head, draft(nodes)).await;

    // The catalogue of the snapshot the draft was saved against, as the daemon reads
    // it back: the criteria must be these and no others.
    let entries = head.context["snapshot"]["catalogue"]["entries"]
        .as_array()
        .expect("the details read carries the snapshot's catalogue")
        .clone();
    assert_eq!(entries.len(), 9, "{entries:?}");
    assert_eq!(
        head.context["snapshot"]["id"],
        next.context["snapshot"]["id"]
    );

    // Jev was asked twice: by `sizing`, then by `pipeline` about both work nodes.
    let requests = stand.jev.requests();
    assert_eq!(requests.len(), 2);
    let (sizing, pipeline) = (&requests[0], &requests[1]);
    assert!(
        asked(sizing)
            .iter()
            .all(|name| !name.ends_with(".pipeline"))
    );
    assert_eq!(asked(pipeline), ["c1.pipeline", "c3.pipeline"]);
    let criteria = pipeline["questions"]["c1.pipeline"]["criteria"]
        .as_object()
        .unwrap();
    assert_eq!(criteria.len(), entries.len() + 1);
    for entry in &entries {
        let text = |field: &str| entry[field].as_str().unwrap();
        assert!(!text("choose_when").is_empty() && !text("expected_result").is_empty());
        assert_eq!(
            criteria[text("kind")],
            json!(format!(
                "Choose when: {} Do not choose when: {} Expected result: {}",
                text("choose_when"),
                text("do_not_choose_when"),
                text("expected_result")
            )),
            "{}",
            text("kind")
        );
    }
    assert!(criteria["ABSTAIN"].is_string());
    // The two nodes chose different pipelines and are offered the same criteria.
    assert_eq!(
        pipeline["questions"]["c3.pipeline"]["criteria"],
        pipeline["questions"]["c1.pipeline"]["criteria"]
    );
    // The state is the one `sizing` sent, and nothing says what the agent chose.
    assert_eq!(pipeline["state"], sizing["state"]);
    assert_eq!(pipeline["state"]["candidates"].as_array().unwrap().len(), 3);
    assert!(!pipeline["state"].to_string().contains("pipeline"));
    let sent = pipeline.to_string();
    for absent in ["REASON-ALPHA", "REASON-BRAVO", "pipeline_reason", KEY] {
        assert!(!sent.contains(absent), "{absent}");
    }

    // The reply carries both fields. The probability for the draft's own choice is
    // the one Jev gave that pipeline, 0.21, and not the share of the other criteria.
    let unnamed = (criteria.len() - 2) as f64;
    let share = ((1.0 - 0.62 - 0.21) / unnamed * 100.0).round() / 100.0;
    assert_ne!(share, 0.21);
    assert_eq!(saved["advice"]["status"], "advised", "{saved}");
    assert_eq!(
        saved["pipeline_advice"],
        json!({"status":"advised","note":PIPELINE_NOTE,"candidates":[
            {"candidate":1,"pipeline":DEBUG,"pipeline_p":0.74,"draft_pipeline_p":0.74,
             "differs":false},
            {"candidate":3,"pipeline":RESEARCH,"pipeline_p":0.62,"draft_pipeline_p":0.21,
             "differs":true}
        ]}),
        "{saved}"
    );
    assert_eq!(next.context["draft"]["nodes"].as_array().unwrap().len(), 3);

    // Two journal rows for the request, one per point, in the order they fired; the
    // `pipeline` row holds exactly the field the agent received and what Jev was sent.
    let written = rows(&stand, request_id).await;
    assert_eq!(points(&written), ["sizing", "pipeline"]);
    let (first, second) = (&written[0], &written[1]);
    assert_eq!(first["advice"], saved["advice"]);
    assert_eq!(second["advice"], saved["pipeline_advice"]);
    assert_eq!(
        second["advice"].to_string(),
        saved["pipeline_advice"].to_string()
    );
    assert_eq!(&second["request"], pipeline);
    assert_eq!(second["route"], "slice.candidates.save");
    assert_eq!(
        (
            &second["called"],
            &second["outcome"],
            &second["send_certainty"]
        ),
        (&json!(true), &json!("advised"), &json!("sent"))
    );
    assert_eq!(second["program_id"], program_id);
    for part in ["candidate_set_id", "scope_id", "program_id", "session_id"] {
        assert!(second[part].is_string(), "{part}");
        assert_eq!(second[part], first[part], "{part}");
    }
    assert_eq!(second["scope_id"], head.context["scope"]["id"]);
    assert!(!json!(written).to_string().contains(KEY));
    client.finish().await;
}
