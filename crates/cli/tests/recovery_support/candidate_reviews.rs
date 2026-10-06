use super::{
    Mcp,
    candidate_reads::{
        CandidateFixture, ReadProvenance, ResolvedRead, read_query_json, read_ready_json,
    },
};
use serde_json::{Value, json};
use std::collections::HashSet;

const MAX_COLLECTION_PAGES: usize = 8192;
const ROUTE: &str = "scope.candidates.context";

pub struct CandidateReviews {
    pub reviews: Vec<Value>,
    pub provenance: Vec<ReadProvenance>,
    pub terminal_page: Value,
}

impl CandidateReviews {
    pub fn terminal_metadata(&self) -> &ReadProvenance {
        self.provenance
            .last()
            .expect("completed review collection provenance")
    }

    pub fn exact(&self, revision: i64) -> &Value {
        self.reviews
            .iter()
            .find(|review| review["revision"] == revision)
            .expect("advertised complete collection must contain expected review revision")
    }

    pub fn latest(&self) -> &Value {
        self.reviews
            .last()
            .expect("complete review collection must contain a review")
    }
}

fn validate_page(fixture: &CandidateFixture, page: &Value, source: Option<&Value>) {
    assert_eq!(page["view"], "reviews", "review view");
    let context = &page["context"];
    for field in ["id", "revision"] {
        assert_eq!(
            context["candidate_set"][field], fixture.mutation["candidate_set"][field],
            "review candidate set {field}"
        );
    }
    for field in ["id", "sequence", "program_revision"] {
        assert_eq!(
            context["snapshot"][field], fixture.mutation["snapshot"][field],
            "review snapshot {field}"
        );
    }
    for field in ["id", "revision", "digest"] {
        assert_eq!(
            context["snapshot"]["method"][field], fixture.mutation["snapshot"]["method"][field],
            "review method {field}"
        );
    }
    if let Some(source) = source {
        assert_eq!(
            source["candidate_set_id"],
            fixture.mutation["candidate_set"]["id"]
        );
        assert_eq!(
            source["candidate_set_revision"],
            fixture.mutation["candidate_set"]["revision"]
        );
        assert_eq!(source["snapshot_id"], fixture.mutation["snapshot"]["id"]);
    }
}

/// Read the complete advertised collection; byte EOF alone is not collection EOF.
pub async fn read_reviews(fixture: &CandidateFixture, client: &mut Mcp) -> CandidateReviews {
    let action = fixture.mutation["field_destinations"]["latest_review"].clone();
    assert_eq!(action["arguments"]["params"]["limit"], 25);
    read_collection(fixture, client, action, true).await
}

/// An independent schema-valid query used to exercise collection pagination.
pub async fn read_reviews_explicit_query(
    fixture: &CandidateFixture,
    client: &mut Mcp,
    limit: i64,
) -> CandidateReviews {
    assert!((1..=100).contains(&limit));
    let arguments = json!({"route": ROUTE, "params": {
        "candidate_set_id": fixture.mutation["candidate_set"]["id"], "view": "reviews", "limit": limit
    }});
    read_collection(fixture, client, arguments, false).await
}

async fn read_collection(
    fixture: &CandidateFixture,
    client: &mut Mcp,
    mut action: Value,
    advertised: bool,
) -> CandidateReviews {
    let arguments = if advertised {
        assert_eq!(action["kind"], "ready_call");
        assert_eq!(action["tool"], "query");
        &action["arguments"]
    } else {
        &action
    };
    assert_eq!(arguments["route"], ROUTE);
    let initial = arguments["params"].clone();
    assert_eq!(
        initial["candidate_set_id"],
        fixture.mutation["candidate_set"]["id"]
    );
    assert_eq!(initial["view"], "reviews");
    assert!(
        initial["limit"]
            .as_i64()
            .is_some_and(|limit| (1..=100).contains(&limit))
    );
    assert!(
        initial["after"].is_null(),
        "review collection must begin at its origin"
    );
    let mut after = 0_i64;
    let mut seen = HashSet::from([after]);
    let mut reviews = Vec::new();
    let mut provenance = Vec::new();
    let mut previous_revision = 0;
    for page_index in 0..MAX_COLLECTION_PAGES {
        let explicit = !advertised && page_index == 0;
        let arguments = if explicit {
            &action
        } else {
            &action["arguments"]
        };
        let limit = arguments["params"]["limit"]
            .as_i64()
            .expect("review page limit");
        assert!((1..=100).contains(&limit), "review page schema limit");
        let read = if explicit {
            read_query_json(client, &action).await
        } else {
            read_ready_json(client, &action).await
        };
        validate_page(fixture, &read.value, read.provenance.source.as_ref());
        let items = read.value["items"]
            .as_array()
            .expect("review collection items");
        assert!(items.len() <= limit as usize, "review page limit");
        for item in items {
            let review = item["review"].as_object().expect("actual review item");
            let revision = review["revision"].as_i64().expect("review revision");
            assert!(
                revision > previous_revision,
                "review revisions must be strictly ascending"
            );
            assert!(
                revision
                    <= fixture.mutation["candidate_set"]["revision"]
                        .as_i64()
                        .unwrap()
            );
            for field in [
                "verdict",
                "summary",
                "findings",
                "candidate_decisions",
                "protected_change_reviews",
            ] {
                assert!(review.contains_key(field), "review field {field}");
            }
            previous_revision = revision;
            reviews.push(item["review"].clone());
        }
        provenance.push(read.provenance);
        let Some(next) = read.value["next_after"].as_i64() else {
            assert!(read.value["next_after"].is_null(), "invalid collection EOF");
            eprintln!(
                "candidate_reviews collection_pages={} reviews={}",
                provenance.len(),
                reviews.len()
            );
            return CandidateReviews {
                reviews,
                provenance,
                terminal_page: read.value,
            };
        };
        assert!(
            !items.is_empty(),
            "continuing review page must advance items"
        );
        assert_eq!(
            next,
            after
                .checked_add(items.len() as i64)
                .expect("review cursor overflow")
        );
        assert!(
            next > after && seen.insert(next),
            "nonprogressing review collection"
        );
        let matching = provenance
            .last()
            .expect("review page provenance")
            .terminal_actions
            .iter()
            .filter(|action| {
                action["kind"] == "ready_call"
                    && action["tool"] == "query"
                    && action["arguments"]["route"] == ROUTE
                    && action["arguments"]["params"]["candidate_set_id"]
                        == initial["candidate_set_id"]
                    && action["arguments"]["params"]["view"] == initial["view"]
                    && action["arguments"]["params"]["limit"]
                        .as_i64()
                        .is_some_and(|limit| (1..=100).contains(&limit))
                    && action["arguments"]["params"]["after"] == next
                    && action["arguments"]["params"].get("offset_bytes").is_none()
            })
            .collect::<Vec<_>>();
        assert_eq!(
            matching.len(),
            1,
            "missing or ambiguous review collection continuation"
        );
        action = matching[0].clone();
        after = next;
    }
    panic!("review collection exceeds finite page budget");
}

/// Select the advertised next semantic source chunk after byte EOF, without executing it.
pub fn source_chunk_next_action(read: &ResolvedRead, cursor: u64) -> Value {
    let initial = &read.provenance.initial_query_arguments["params"];
    assert_eq!(initial["view"], "fragment");
    let matching = read
        .provenance
        .terminal_actions
        .iter()
        .filter(|action| {
            let arguments = &action["arguments"];
            let params = &arguments["params"];
            action["kind"] == "ready_call"
                && action["tool"] == "query"
                && arguments["route"] == ROUTE
                && params["view"] == "fragment"
                && params["candidate_set_id"] == initial["candidate_set_id"]
                && params["source_ref_id"] == initial["source_ref_id"]
                && params["cursor"] == cursor
                && params.get("draft_revision") == initial.get("draft_revision")
                && params.get("offset_bytes").is_none()
        })
        .collect::<Vec<_>>();
    assert_eq!(matching.len(), 1, "actual semantic source continuation");
    matching[0].clone()
}
