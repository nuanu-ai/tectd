//! A ready review refused over Jev's open advice, as the agent reads it: the intro
//! with the two ways out, the open items, the read of the draft, and its size.
use super::*;
use tect_domain::{BindingPoint, OpenAdvice, SizingLevel};
use uuid::Uuid;

/// This many candidates of a draft with open `sizing` advice and nothing stated.
fn refused(level: SizingLevel, candidates: u128) -> Error {
    let open = (1..=candidates)
        .map(|candidate| OpenAdvice {
            id: Uuid::from_u128(1000 + candidate),
            candidate_id: Uuid::from_u128(candidate),
            candidate_revision: 1,
            point: BindingPoint::Sizing,
            answer: json!({"advice":"separate_scope","extra_items":[2, 5]}),
        })
        .collect::<Vec<_>>();
    let draft = (1..=candidates)
        .map(|candidate| (Uuid::from_u128(candidate), 1))
        .collect::<Vec<_>>();
    level
        .review_departures(true, &draft, &open, &[])
        .unwrap_err()
}

/// The review call of each level that the refusal answers.
fn calls() -> [(SizingLevel, &'static str, Value, &'static str); 2] {
    let id = Uuid::from_u128(5);
    let review = json!({"verdict":"ready","summary":"Bounded and ready","findings":[]});
    [
        (
            SizingLevel::ProgramToScopes,
            "scope.candidates.save",
            json!({"route":"scope.candidates.save","params":{"kind":"review",
                "candidate_set_id":id,"revision":2,"snapshot_id":id,"input_cursor":1,
                "request_id":id,"review":review}}),
            "scope.candidates.context",
        ),
        (
            SizingLevel::ScopeToSlices,
            "slice.candidates.save",
            json!({"route":"slice.candidates.save","params":{"kind":"review","scope_id":id,
                "candidate_set_id":id,"revision":2,"snapshot_id":id,"input_cursor":1,
                "request_id":id,"review":review}}),
            "slice.candidates.context",
        ),
    ]
}

#[test]
fn a_refused_ready_review_names_the_open_items_the_two_ways_out_and_the_draft_to_read() {
    for (level, route, arguments, read) in calls() {
        let response = failure(refused(level, 2), Some(("command", &arguments)));
        assert_eq!(response["isError"], true);
        let intro = response["content"][0]["text"].as_str().unwrap();
        let mut from = 0;
        for step in [
            "This review is not stored",
            "a review says ready only when that advice is followed",
            "change it as the advice says and save the draft again with command route",
            route,
            "where the advice about it is plainly wrong",
            "add an entry to advice_departures of this review",
            "the candidate's id, the advice point and your reason",
        ] {
            let found = intro[from..]
                .find(step)
                .unwrap_or_else(|| panic!("`{step}` is missing or out of order in: {intro}"));
            from += found + step.len();
        }
        assert!(intro.ends_with("Nothing was stored."), "{intro}");
        let body: Value =
            serde_json::from_str(response["content"][1]["text"].as_str().unwrap()).unwrap();
        assert_eq!(body["error"]["code"], "ADVICE_NOT_FOLLOWED");
        let refusal = &body["error"]["refusal"];
        assert_eq!(refusal["code"], "ADVICE_NOT_FOLLOWED");
        assert_eq!(refusal["path"], "/params/review/advice_departures");
        assert_eq!(refusal["next_action"], route);
        let named = refusal["actual"].as_str().unwrap();
        for candidate in [1, 2] {
            let item = format!(
                "candidate {} sizing: separate_scope, extra items 2, 5",
                Uuid::from_u128(candidate)
            );
            assert!(named.contains(&item), "{named}");
        }
        // The one action reads the candidate set, where the draft and its ids are.
        assert_eq!(body["actions"].as_array().unwrap().len(), 1);
        assert_eq!(body["actions"][0]["arguments"]["route"], read);
        assert_eq!(
            body["actions"][0]["arguments"]["params"]["view"],
            "overview"
        );
        assert_eq!(body["error"]["schema_help"]["arguments"]["route"], route);
    }
}

#[test]
fn the_longest_list_of_open_items_leaves_the_refusal_whole_within_its_budget() {
    for (level, route, arguments, _) in calls() {
        // A hundred open items: more than the refusal names.
        let error = refused(level, 100);
        let named = error.refusal().unwrap().actual.unwrap();
        assert!(named.contains("; and "), "{named}");
        let response = failure_bounded(error, Some(("command", &arguments)), None, 8192).unwrap();
        let bytes = serde_json::to_vec(&response).unwrap().len();
        println!(
            "{route}: refusal over 100 open items, envelope_bytes={bytes}, named_bytes={}",
            named.len()
        );
        assert!(bytes <= 8192, "{bytes}");
        // Nothing was dropped to make it fit.
        let body: Value =
            serde_json::from_str(response["content"][1]["text"].as_str().unwrap()).unwrap();
        assert!(body["error"].get("diagnostic_delivery").is_none());
        assert_eq!(body["error"]["refusal"]["actual"], named);
        assert!(body["error"]["schema_help"].is_object());
        assert_eq!(body["actions"].as_array().unwrap().len(), 1);
    }
}
