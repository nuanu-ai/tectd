//! What the two save routes tell an agent about departing from Jev's advice, and
//! what their parsers take for a departure.
use crate::api::{help, parse_help};
use crate::scope_candidate_tools::{self, ScopeCandidateInvocation};
use crate::slice_tools::{self, SliceInvocation};
use serde_json::{Value, json};
use tect_domain::BindingPoint;

fn describe(route: &str) -> Value {
    help(parse_help(json!({"mode":"describe","tool":"command","route":route})).unwrap()).unwrap()
}

/// A review of each level as an agent sends it, with this `advice_departures`.
fn reviews(departures: &Value) -> (Value, Value) {
    let id = "00000000-0000-4000-8000-000000000001";
    let mut scope = json!({"kind":"review","candidate_set_id":id,"revision":2,
        "snapshot_id":id,"input_cursor":1,"request_id":id,"review":{"verdict":"ready",
            "summary":"reviewed","findings":[],"candidate_decisions":[]}});
    let mut slice = json!({"kind":"review","scope_id":id,"candidate_set_id":id,"revision":2,
        "snapshot_id":id,"input_cursor":1,"request_id":id,
        "review":{"verdict":"ready","summary":"reviewed"}});
    scope["review"]["advice_departures"] = departures.clone();
    slice["review"]["advice_departures"] = departures.clone();
    (scope, slice)
}

#[test]
fn the_review_of_both_save_routes_describes_advice_departures() {
    for route in ["scope.candidates.save", "slice.candidates.save"] {
        let described = describe(route);
        let review = &described["params_schema"]["oneOf"][1]["properties"]["review"];
        assert_eq!(
            described["params_schema"]["oneOf"][1]["properties"]["kind"]["const"],
            "review"
        );
        let stated = &review["properties"]["advice_departures"];
        assert_eq!(stated["type"], "array", "{route}");
        let item = &stated["items"];
        assert_eq!(item["additionalProperties"], false);
        assert_eq!(item["required"], json!(["candidate_id", "point", "reason"]));
        assert_eq!(
            item["properties"]["point"]["enum"],
            json!([
                BindingPoint::Sizing.as_str(),
                BindingPoint::Pipeline.as_str()
            ])
        );
        assert_eq!(item["properties"]["candidate_id"]["format"], "uuid");
        assert_eq!(item["properties"]["reason"]["minLength"], 1);
        // Stated or left out: a review does not have to carry it.
        assert!(
            !review["required"]
                .as_array()
                .unwrap()
                .contains(&json!("advice_departures"))
        );
        let about = stated["description"].as_str().unwrap();
        for said in [
            "is to be followed",
            "change the draft as it says and save it again",
            "only where the advice about one candidate is plainly wrong",
            "reason says why that advice is wrong",
            "whose verdict is ready is refused with ADVICE_NOT_FOLLOWED",
            "null is invalid",
        ] {
            assert!(about.contains(said), "{said}: {about}");
        }
        let conditions = described["conditions"].as_str().unwrap();
        assert!(
            conditions.contains("A review may state advice_departures"),
            "{conditions}"
        );
        let effects = described["effects"].as_str().unwrap();
        for said in [
            "While Jev is enabled, a review whose verdict is ready is refused with \
             ADVICE_NOT_FOLLOWED and nothing is stored",
            "the review states no departure for it",
            "a stated departure is recorded with the review",
        ] {
            assert!(effects.contains(said), "{said}: {effects}");
        }
        // The draft of the same route takes no such list.
        let draft = &described["params_schema"]["oneOf"][0]["properties"]["draft"];
        assert!(draft["properties"].get("advice_departures").is_none());
    }
    // The two planning methods are not edited for it.
    for method in ["tectd-scope-candidates", "tectd-slice-candidates"] {
        let body = help(parse_help(json!({"mode":"describe","method":method})).unwrap()).unwrap();
        assert!(!body["body"].as_str().unwrap().contains("advice_departures"));
    }
}

#[test]
fn both_parsers_take_a_stated_list_and_refuse_what_is_not_one() {
    let id = "00000000-0000-4000-8000-000000000001";
    let stated = json!([{"candidate_id":id,"point":"pipeline","reason":"The cause is known"}]);
    let (scope, slice) = reviews(&stated);
    let Ok(ScopeCandidateInvocation::Review(request)) =
        scope_candidate_tools::parse("save_candidate_set", scope)
    else {
        panic!("a Scope review with a departure");
    };
    let departures = request.review.advice_departures.unwrap();
    assert_eq!(departures[0].point, BindingPoint::Pipeline);
    assert_eq!(departures[0].reason, "The cause is known");
    let Ok(SliceInvocation::Review(request)) =
        slice_tools::parse("save_slice_candidate_set", slice)
    else {
        panic!("a Slice review with a departure");
    };
    assert_eq!(request.review.advice_departures.unwrap(), departures);

    for refused in [
        Value::Null,
        json!([{"candidate_id":id,"point":"intake","reason":"r"}]),
        json!([{"candidate_id":id,"point":"sizing"}]),
        json!([{"candidate_id":id,"point":"sizing","reason":"r","why":"x"}]),
    ] {
        let (scope, slice) = reviews(&refused);
        let codes = [
            scope_candidate_tools::parse("save_candidate_set", scope)
                .err()
                .map(|error| error.code()),
            slice_tools::parse("save_slice_candidate_set", slice)
                .err()
                .map(|error| error.code()),
        ];
        assert_eq!(codes, [Some("invalid_arguments"); 2], "{refused}");
    }
}
