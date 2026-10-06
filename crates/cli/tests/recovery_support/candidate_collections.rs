use super::{
    Mcp,
    candidate_reads::{ReadProvenance, ResolvedRead, read_query_json, read_ready_json},
};
use serde_json::Value;
use std::collections::HashSet;

const ROUTE: &str = "scope.candidates.context";
const MAX_COLLECTION_PAGES: usize = 8192;
const MAX_COLLECTION_ITEMS: usize = 100_000;
const MAX_COLLECTION_BYTES: usize = 32 * 1024 * 1024;

pub struct CollectionEvidence {
    pub pages: Vec<ResolvedRead>,
    pub items: Vec<Value>,
}

impl CollectionEvidence {
    pub fn terminal(&self) -> &ReadProvenance {
        &self
            .pages
            .last()
            .expect("complete collection EOF")
            .provenance
    }
}

fn coherent(first: &Value, read: &ResolvedRead, initial: &Value) {
    let context = &read.value["context"];
    assert_eq!(context["candidate_set"]["id"], initial["candidate_set_id"]);
    for field in [
        "id",
        "revision",
        "program_id",
        "current_snapshot_id",
        "input_cursor",
        "latest_input",
    ] {
        assert!(
            context["candidate_set"].get(field).is_some(),
            "set pin {field}"
        );
        assert_eq!(
            context["candidate_set"][field], first["candidate_set"][field],
            "set pin {field}"
        );
    }
    for field in [
        "id",
        "sequence",
        "program_revision",
        "program_latest_input",
        "planning_latest_input",
    ] {
        assert!(
            context["snapshot"].get(field).is_some(),
            "snapshot pin {field}"
        );
        assert_eq!(
            context["snapshot"][field], first["snapshot"][field],
            "snapshot pin {field}"
        );
    }
    for field in ["id", "revision", "digest"] {
        assert!(
            context["snapshot"]["method"].get(field).is_some(),
            "method pin {field}"
        );
        assert_eq!(
            context["snapshot"]["method"][field], first["snapshot"]["method"][field],
            "method pin {field}"
        );
    }
    assert!(context.get("current_program_revision").is_some());
    assert_eq!(
        context["current_program_revision"],
        first["current_program_revision"]
    );
    if let Some(source) = &read.provenance.source {
        assert_eq!(source["candidate_set_id"], context["candidate_set"]["id"]);
        assert_eq!(
            source["candidate_set_revision"],
            context["candidate_set"]["revision"]
        );
        assert_eq!(source["snapshot_id"], context["snapshot"]["id"]);
    }
}

/// Keep each actual page and aggregate only its collection items; never hydrate a DTO.
pub async fn read_collection_query(client: &mut Mcp, arguments: &Value) -> CollectionEvidence {
    assert_eq!(arguments["route"], ROUTE);
    let initial = &arguments["params"];
    assert!(matches!(
        initial["view"].as_str(),
        Some("history" | "candidates" | "inputs")
    ));
    let mut after = initial
        .get("after")
        .map_or(0, |value| value.as_i64().expect("initial cursor"));
    assert!(after >= 0);
    let mut seen = HashSet::from([after]);
    let mut current_action = None;
    let mut pages: Vec<ResolvedRead> = Vec::new();
    let mut items = Vec::new();
    let mut input_sequence = 0;
    let mut representation_bytes = 0_usize;
    for _ in 0..MAX_COLLECTION_PAGES {
        let read = if let Some(action) = current_action.take() {
            read_ready_json(client, &action).await
        } else {
            read_query_json(client, arguments).await
        };
        assert_eq!(read.value["view"], initial["view"]);
        representation_bytes = representation_bytes
            .checked_add(
                serde_json::to_vec(&read.value)
                    .expect("actual page JSON")
                    .len(),
            )
            .expect("collection byte budget overflow");
        assert!(
            representation_bytes <= MAX_COLLECTION_BYTES,
            "finite collection representation budget"
        );
        let first = pages
            .first()
            .map_or(&read.value["context"], |page| &page.value["context"]);
        coherent(first, &read, initial);
        let page_items = read.value["items"].as_array().expect("collection items");
        let params = &read.provenance.initial_query_arguments["params"];
        let limit = params["limit"].as_i64().expect("page limit");
        assert!((1..=100).contains(&limit) && page_items.len() <= limit as usize);
        if initial["view"] == "inputs" {
            for item in page_items {
                let sequence = item["input"]["sequence"].as_i64().expect("input sequence");
                assert!(sequence > input_sequence, "input item cursor must advance");
                input_sequence = sequence;
            }
        }
        assert!(
            items
                .len()
                .checked_add(page_items.len())
                .is_some_and(|count| count <= MAX_COLLECTION_ITEMS)
        );
        items.extend(page_items.iter().cloned());
        let next = read.value["next_after"].as_i64();
        if let Some(next) = next {
            assert!(
                !page_items.is_empty(),
                "collection cursor cannot skip empty page"
            );
            assert!(
                next > after && seen.insert(next),
                "collection cursor must advance once"
            );
            let matching = read
                .provenance
                .terminal_actions
                .iter()
                .filter(|action| {
                    let params = &action["arguments"]["params"];
                    action["kind"] == "ready_call"
                        && action["tool"] == "query"
                        && action["arguments"]["route"] == ROUTE
                        && params["candidate_set_id"] == initial["candidate_set_id"]
                        && params["view"] == initial["view"]
                        && params.get("draft_revision") == initial.get("draft_revision")
                        && params["after"] == next
                        && params["limit"]
                            .as_i64()
                            .is_some_and(|limit| (1..=100).contains(&limit))
                        && params.get("offset_bytes").is_none()
                })
                .collect::<Vec<_>>();
            assert_eq!(matching.len(), 1, "actual collection continuation");
            current_action = Some(matching[0].clone());
            after = next;
        } else {
            assert!(read.value["next_after"].is_null(), "collection EOF");
            pages.push(read);
            eprintln!(
                "candidate_collection view={} pages={} items={}",
                initial["view"],
                pages.len(),
                items.len()
            );
            return CollectionEvidence { pages, items };
        }
        pages.push(read);
    }
    panic!("candidate collection exceeds finite budget");
}
