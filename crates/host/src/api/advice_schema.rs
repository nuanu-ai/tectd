use super::catalog::RouteSpec;
use crate::tools::object_schema;
use serde_json::{Value, json};
use tect_domain::MAX_ADVICE_PAGE;

fn configure() -> Value {
    object_schema(
        json!({
            "enabled":{"type":"boolean"},
            "daily_call_limit":{"type":"integer","minimum":1},
            "expected_revision":{"type":"integer","minimum":0}
        }),
        json!(["enabled", "expected_revision"]),
    )
}

fn audit() -> Value {
    let time = json!({"type":"string","format":"date-time"});
    crate::planning_read::schema(object_schema(
        json!({
            "point":{"type":"string","pattern":"^[a-z0-9_]{1,64}$"},
            "outcome":{"type":"string","enum":["advised","abstained","unavailable"]},
            "since":time.clone(),
            "until":time,
            "after":{"type":"string","format":"uuid"},
            "limit":{"type":"integer","minimum":0,"maximum":MAX_ADVICE_PAGE},
            "bodies":{"type":"boolean"}
        }),
        json!(["limit", "bodies"]),
    ))
}

/// The departures from Jev's advice that a review states, on both save routes.
pub(super) fn departures() -> Value {
    let departure = object_schema(
        json!({
            "candidate_id":{"type":"string","format":"uuid"},
            "point":{"type":"string","enum":["sizing","pipeline"]},
            "reason":{"type":"string","minLength":1}
        }),
        json!(["candidate_id", "point", "reason"]),
    );
    json!({"type":"array","items":departure,"description":DEPARTURES})
}

const DEPARTURES: &str = "Jev's advice this review departs from. The advice on a saved draft is to be followed: change the draft as it says and save it again. State a departure only where the advice about one candidate is plainly wrong: candidate_id is that candidate's id in the saved draft, point is the advice point (sizing for the advice field of the draft save, pipeline for its pipeline_advice field) and reason says why that advice is wrong. One candidate and point is named once. Omit the field when the review departs from nothing: null is invalid.";

pub(super) fn routes() -> Vec<RouteSpec> {
    vec![
        RouteSpec {
            tool: "query",
            route: "workspace.advisory.config",
            internal: "advice_config",
            summary: "Read this workspace's Jev advice setting, today's call count, and whether the daemon has a Jev provider.",
            conditions: "Requires an authenticated open native session.",
            effects: "Reads the setting and counts today's journal rows, by UTC day, whose request went out. A workspace without a setting reads as enabled false at revision 0. It never calls Jev.",
            retry: "Safe to repeat.",
            schema: object_schema(json!({}), json!([])),
            example: json!({}),
        },
        RouteSpec {
            tool: "command",
            route: "workspace.advisory.configure",
            internal: "advice_configure",
            summary: "Enable or disable Jev advice for this workspace and set its daily call limit.",
            conditions: "Requires an authenticated open native session and the revision read from workspace.advisory.config; 0 means the workspace has no setting yet. enabled true requires daily_call_limit: there is no default limit.",
            effects: "Atomically writes the workspace's single setting and advances its revision. An omitted daily_call_limit keeps the stored one. While disabled, no command calls Jev, writes a journal row, or returns an advice field.",
            retry: "A stale expected_revision is refused and nothing changes. Read workspace.advisory.config and retry with its revision.",
            schema: configure(),
            example: json!({"enabled":true,"daily_call_limit":50,"expected_revision":0}),
        },
        RouteSpec {
            tool: "query",
            route: "workspace.advisory.audit",
            internal: "advice_audit",
            summary: "Read this workspace's Jev advice journal, newest first, with counters of calls, skips and their reasons.",
            conditions: "Requires an authenticated open native session. point, outcome, since and until select rows; since is inclusive, until is exclusive, both RFC 3339. limit 0 returns the counters alone. bodies is required and has no default. Explicit null is invalid.",
            effects: "Reads journal rows. bodies true includes request_body and response_body, null where none was recorded; bodies false leaves both fields out of every row, and request_bytes and response_bytes still give their sizes. Counters cover the point and time filters and ignore outcome and paging. A page over the read budget is delivered as lossless JSON byte windows.",
            retry: "Safe to repeat. Follow next_after unchanged; give until so that later rows do not move the page between windows.",
            schema: audit(),
            example: json!({"limit":5,"point":"sizing","bodies":false}),
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn three_routes_are_described_and_decoded_by_the_registry() {
        let routes = routes();
        assert_eq!(
            routes
                .iter()
                .map(|spec| (spec.tool, spec.route))
                .collect::<Vec<_>>(),
            [
                ("query", "workspace.advisory.config"),
                ("command", "workspace.advisory.configure"),
                ("query", "workspace.advisory.audit"),
            ]
        );
        for spec in &routes {
            let described = crate::api::help(
                crate::api::parse_help(
                    json!({"mode":"describe","tool":spec.tool,"route":spec.route}),
                )
                .unwrap(),
            )
            .unwrap();
            assert_eq!(described["params_schema"], spec.schema);
            let call = crate::api::decode_public_call(
                spec.tool,
                json!({"route":spec.route,"params":spec.example}),
            )
            .unwrap();
            assert_eq!(call.name, spec.internal);
            assert_eq!(
                crate::api::read_only_internal_call(spec.internal),
                spec.tool == "query"
            );
        }
        assert_eq!(
            routes[1].schema["properties"]["daily_call_limit"]["minimum"],
            1
        );
        assert!(
            routes[1].schema["properties"]["daily_call_limit"]
                .get("default")
                .is_none()
        );
        assert_eq!(routes[2].schema["properties"]["limit"]["maximum"], 100);
        assert_eq!(routes[2].schema["required"], json!(["limit", "bodies"]));
        assert!(
            routes[2].schema["properties"]["bodies"]
                .get("default")
                .is_none()
        );
        assert_eq!(
            routes[2].schema["properties"]["limit_bytes"]["maximum"],
            4096
        );
        for (tool, route, params) in [
            ("command", "workspace.advisory.config", json!({})),
            (
                "query",
                "workspace.advisory.configure",
                json!({"enabled":false,"expected_revision":0}),
            ),
            (
                "command",
                "workspace.advisory.configure",
                json!({"enabled":true,"expected_revision":0}),
            ),
            (
                "query",
                "workspace.advisory.audit",
                json!({"limit":101,"bodies":true}),
            ),
            ("query", "workspace.advisory.audit", json!({"limit":5})),
        ] {
            assert!(
                crate::api::decode_public_call(tool, json!({"route":route,"params":params}))
                    .is_err()
            );
        }
        for (text, route) in [
            ("журнал джева", "workspace.advisory.audit"),
            ("включить джева", "workspace.advisory.configure"),
            ("jev advice setting", "workspace.advisory.config"),
        ] {
            let found = crate::api::help(
                crate::api::parse_help(json!({"mode":"search","text":text})).unwrap(),
            )
            .unwrap();
            assert_eq!(found["hits"][0]["route"], route, "{text}");
        }
    }
}

#[cfg(test)]
mod departure_tests;
