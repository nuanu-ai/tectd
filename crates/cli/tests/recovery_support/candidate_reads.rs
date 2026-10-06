use super::Mcp;
use serde_json::Value;
use sha2::{Digest, Sha256};
use uuid::Uuid;

const MAX_BYTES: usize = 8 * 1024 * 1024;
const MAX_PAGES: usize = 8192;
const ROUTE: &str = "scope.candidates.context";

pub struct ReadProvenance {
    pub initial_action: Option<Value>,
    pub initial_query_arguments: Value,
    pub source: Option<Value>,
    pub representation_digest: Option<String>,
    pub pages: usize,
    pub maximum_payload_bytes: usize,
    pub terminal_actions: Vec<Value>,
    pub terminal_recommended_action: Option<usize>,
}

pub struct ResolvedRead {
    pub value: Value,
    pub provenance: ReadProvenance,
}

fn action_params(action: &Value) -> &Value {
    assert_eq!(action["kind"], "ready_call", "read action must be ready");
    assert_eq!(action["tool"], "query", "read action must be a query");
    query_params(&action["arguments"])
}

fn query_params(arguments: &Value) -> &Value {
    let route = arguments["route"].as_str().expect("read route");
    assert!(
        matches!(route, ROUTE | "scope.context" | "slice.candidates.context"),
        "unsupported read route"
    );
    let params = &arguments["params"];
    assert!(params.is_object(), "read action parameters missing");
    let identity = if route == ROUTE {
        "candidate_set_id"
    } else {
        "scope_id"
    };
    Uuid::parse_str(params[identity].as_str().expect("read identity")).expect("read UUID");
    if route != "scope.context" {
        assert!(params["view"].is_string(), "read view missing");
    }
    if route == ROUTE && params["view"] == "fragment" {
        Uuid::parse_str(
            params["source_ref_id"]
                .as_str()
                .expect("fragment source query ID"),
        )
        .expect("source query UUID");
        assert!(
            params["cursor"].as_u64().is_some(),
            "semantic source cursor"
        );
        assert!(
            params.get("candidate_set_revision").is_none(),
            "source fragment must not pin set revision"
        );
        if let Some(revision) = params.get("draft_revision") {
            assert!(
                revision.as_i64().is_some_and(|v| v > 0),
                "historical draft revision"
            );
        }
    }
    params
}

fn terminal_metadata(page: &Value) -> (Vec<Value>, Option<usize>) {
    let actions = page
        .get("actions")
        .and_then(Value::as_array)
        .expect("actual terminal actions")
        .clone();
    let recommendation = page
        .get("recommended_action")
        .expect("actual recommendation present");
    let recommended = if recommendation.is_null() {
        None
    } else {
        let index = usize::try_from(recommendation.as_u64().expect("recommendation index"))
            .expect("recommendation bounds");
        assert!(
            index < actions.len(),
            "recommendation indexes actual actions"
        );
        Some(index)
    };
    (actions, recommended)
}

/// Executes advertised queries and verifies the original byte representation.
/// EOF ends byte assembly; collection continuations remain the caller's job.
pub async fn read_ready_json(client: &mut Mcp, action: &Value) -> ResolvedRead {
    action_params(action);
    resolve_query(client, &action["arguments"], Some(action)).await
}

/// Executes the caller's actual public query; no advertised action is synthesized.
pub async fn read_query_json(client: &mut Mcp, arguments: &Value) -> ResolvedRead {
    resolve_query(client, arguments, None).await
}

async fn resolve_query(
    client: &mut Mcp,
    arguments: &Value,
    advertised: Option<&Value>,
) -> ResolvedRead {
    let initial_params = query_params(arguments).clone();
    let route = arguments["route"].as_str().expect("read route");
    let source_fragment = route == ROUTE && initial_params["view"] == "fragment";
    assert!(
        initial_params.get("offset_bytes").is_none(),
        "initial byte offset"
    );
    let mut current = arguments.clone();
    let mut bytes = Vec::new();
    let mut source = None;
    let mut digest: Option<String> = None;
    let mut total = None;
    let mut maximum = 0;
    for pages in 1..=MAX_PAGES {
        let params = query_params(&current);
        for (key, value) in initial_params.as_object().expect("initial params") {
            assert_eq!(params.get(key), Some(value), "changed read selector {key}");
        }
        let page = client.call("query", current.clone()).await;
        let payload_bytes = serde_json::to_vec(&page).expect("read JSON").len();
        assert!(
            payload_bytes <= MAX_BYTES,
            "fixture read exceeds byte budget"
        );
        maximum = maximum.max(payload_bytes);
        if page["kind"] != "fragment" {
            assert!(
                bytes.is_empty() && digest.is_none(),
                "fragment became ordinary page"
            );
            verify_read_value(&page, &initial_params, route, source_fragment, None);
            let (terminal_actions, terminal_recommended_action) = terminal_metadata(&page);
            return ResolvedRead {
                value: page,
                provenance: ReadProvenance {
                    initial_action: advertised.cloned(),
                    initial_query_arguments: arguments.clone(),
                    source: None,
                    representation_digest: None,
                    pages,
                    maximum_payload_bytes: maximum,
                    terminal_actions,
                    terminal_recommended_action,
                },
            };
        }
        assert_eq!(page["format"], "json", "fragment format");
        assert_eq!(page["encoding"], "utf-8", "fragment encoding");
        let page_source = page["source"].as_object().expect("fragment source");
        let identity = if route == ROUTE {
            "candidate_set_id"
        } else {
            "scope_id"
        };
        assert_eq!(
            page_source.get(identity),
            initial_params.get(identity),
            "fragment identity pin"
        );
        let uuid_pins: &[&str] = match route {
            ROUTE if source_fragment => &["candidate_set_id", "source_ref_id"],
            ROUTE => &["candidate_set_id", "snapshot_id"],
            "scope.context" => &["scope_id"],
            "slice.candidates.context" => &["scope_id", "candidate_set_id", "snapshot_id"],
            _ => unreachable!(),
        };
        for pin in uuid_pins {
            Uuid::parse_str(page["source"][*pin].as_str().expect("source UUID pin"))
                .expect("source UUID");
        }
        let revision_pin = match route {
            ROUTE if source_fragment => None,
            ROUTE => Some("candidate_set_revision"),
            "scope.context" => Some("scope_revision"),
            _ => None,
        };
        if let Some(pin) = revision_pin {
            assert!(
                page["source"][pin].as_i64().is_some_and(|r| r > 0),
                "source revision"
            );
        }
        if source_fragment {
            assert_eq!(
                page["source"]["source_ref_id"], initial_params["source_ref_id"],
                "semantic source pin"
            );
            let revision = page["source"]
                .get("draft_revision")
                .expect("actual source draft revision field");
            if let Some(requested) = initial_params.get("draft_revision") {
                assert_eq!(revision, requested, "historical source pin");
            } else {
                assert!(
                    revision.is_null() || revision.as_i64().is_some_and(|v| v > 0),
                    "actual draft revision pin"
                );
            }
        }
        let page_digest = page["representation_digest"]
            .as_str()
            .expect("fragment digest");
        assert!(
            page_digest.len() == 64
                && page_digest
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
            "digest syntax"
        );
        let page_total = usize::try_from(page["total_bytes"].as_u64().expect("fragment total"))
            .expect("total bounds");
        assert!(page_total <= MAX_BYTES, "fixture read exceeds byte budget");
        if let Some(expected) = &source {
            assert_eq!(&page["source"], expected, "changed fragment source");
            assert_eq!(Some(page_digest), digest.as_deref(), "changed digest");
            assert_eq!(Some(page_total), total, "changed total");
        } else {
            source = Some(page["source"].clone());
            digest = Some(page_digest.to_owned());
            total = Some(page_total);
        }
        let offset = usize::try_from(page["offset_bytes"].as_u64().expect("fragment offset"))
            .expect("offset bounds");
        assert_eq!(offset, bytes.len(), "noncontiguous fragment");
        let text = page["text"].as_str().expect("fragment UTF-8 text");
        let returned = usize::try_from(
            page["returned_bytes"]
                .as_u64()
                .expect("fragment byte count"),
        )
        .expect("count bounds");
        assert_eq!(returned, text.len(), "fragment byte count mismatch");
        let end = offset
            .checked_add(returned)
            .expect("fragment byte overflow");
        assert!(end <= page_total, "fragment exceeds total");
        bytes.extend_from_slice(text.as_bytes());
        if page["next_offset_bytes"].is_null() {
            assert_eq!(end, page_total, "premature fragment EOF");
            assert_eq!(
                format!("{:x}", Sha256::digest(&bytes)),
                page_digest,
                "assembled byte digest"
            );
            let value = serde_json::from_slice(&bytes).expect("complete JSON representation");
            verify_read_value(
                &value,
                &initial_params,
                route,
                source_fragment,
                Some(&page["source"]),
            );
            let (terminal_actions, terminal_recommended_action) = terminal_metadata(&page);
            eprintln!("candidate_read pages={pages} bytes={end} maximum_payload_bytes={maximum}");
            return ResolvedRead {
                value,
                provenance: ReadProvenance {
                    initial_action: advertised.cloned(),
                    initial_query_arguments: arguments.clone(),
                    source,
                    representation_digest: digest,
                    pages,
                    maximum_payload_bytes: maximum,
                    terminal_actions,
                    terminal_recommended_action,
                },
            };
        }
        assert!(returned > 0, "fragment failed to advance");
        assert_eq!(
            page["next_offset_bytes"].as_u64(),
            Some(end as u64),
            "next fragment offset"
        );
        let actions = page["actions"].as_array().expect("fragment actions");
        let matching = actions
            .iter()
            .filter(|next| {
                next["kind"] == "ready_call"
                    && next["tool"] == "query"
                    && next["arguments"]["route"] == arguments["route"]
                    && next["arguments"]["params"]["offset_bytes"].as_u64() == Some(end as u64)
                    && next["arguments"]["params"]["representation_digest"]
                        == page["representation_digest"]
                    && initial_params
                        .as_object()
                        .unwrap()
                        .iter()
                        .all(|(key, value)| next["arguments"]["params"].get(key) == Some(value))
            })
            .collect::<Vec<_>>();
        assert_eq!(matching.len(), 1, "missing or ambiguous byte continuation");
        let next = matching[0];
        let next_params = action_params(next);
        assert_eq!(
            next_params[identity], page["source"][identity],
            "continuation identity"
        );
        if route == ROUTE && !source_fragment {
            assert_eq!(
                next_params["candidate_set_revision"], page["source"]["candidate_set_revision"],
                "continuation revision"
            );
        }
        current = next["arguments"].clone();
    }
    panic!("fixture read exceeds page budget");
}

fn verify_read_value(
    value: &Value,
    params: &Value,
    route: &str,
    source_fragment: bool,
    source: Option<&Value>,
) {
    if source_fragment {
        assert_eq!(
            value["fragment"]["source_ref"]["id"], params["source_ref_id"],
            "source DTO ID"
        );
        assert_eq!(
            value["fragment"]["cursor"], params["cursor"],
            "source DTO cursor"
        );
    } else if route == ROUTE {
        assert_eq!(
            value["context"]["candidate_set"]["id"], params["candidate_set_id"],
            "context DTO ID"
        );
        if let Some(source) = source {
            assert_eq!(
                value["context"]["candidate_set"]["revision"], source["candidate_set_revision"],
                "context DTO revision"
            );
            assert_eq!(
                value["context"]["snapshot"]["id"], source["snapshot_id"],
                "context DTO snapshot"
            );
        }
    }
}

pub struct CandidateFixture {
    pub mutation: Value,
}

impl CandidateFixture {
    pub fn from_mutation(mutation: Value) -> Self {
        assert!(
            mutation.get("context").is_none(),
            "mutation must stay compact"
        );
        assert!(mutation.get("draft").is_none(), "mutation must omit draft");
        let set = &mutation["candidate_set"];
        Uuid::parse_str(set["id"].as_str().expect("mutation set ID")).expect("mutation set UUID");
        assert!(
            set["revision"].as_i64().is_some_and(|r| r > 0),
            "mutation revision"
        );
        let snapshot = &mutation["snapshot"];
        Uuid::parse_str(snapshot["id"].as_str().expect("mutation snapshot ID"))
            .expect("snapshot UUID");
        for name in ["sequence", "program_revision"] {
            assert!(
                snapshot[name].as_i64().is_some_and(|r| r > 0),
                "mutation snapshot {name}"
            );
        }
        assert!(snapshot["method"]["id"].is_string(), "mutation method ID");
        assert!(
            snapshot["method"]["revision"].is_string(),
            "mutation method revision"
        );
        assert!(
            snapshot["method"]["digest"].is_string(),
            "mutation method digest"
        );
        Self { mutation }
    }

    async fn read(&self, client: &mut Mcp, destination: &str, view: &str) -> ResolvedRead {
        let action = &self.mutation["field_destinations"][destination];
        let params = action_params(action);
        assert_eq!(
            params["candidate_set_id"], self.mutation["candidate_set"]["id"],
            "destination set"
        );
        assert_eq!(params["view"], view, "destination view");
        let read = read_ready_json(client, action).await;
        assert_eq!(read.value["view"], view, "returned view");
        let context = &read.value["context"];
        assert_eq!(
            context["candidate_set"]["id"], self.mutation["candidate_set"]["id"],
            "read set ID"
        );
        assert_eq!(
            context["candidate_set"]["revision"], self.mutation["candidate_set"]["revision"],
            "read set revision"
        );
        for name in ["id", "sequence", "program_revision"] {
            assert_eq!(
                context["snapshot"][name], self.mutation["snapshot"][name],
                "read snapshot {name}"
            );
        }
        for name in ["id", "revision", "digest"] {
            assert_eq!(
                context["snapshot"]["method"][name], self.mutation["snapshot"]["method"][name],
                "read method {name}"
            );
        }
        if let Some(source) = &read.provenance.source {
            assert_eq!(
                source["candidate_set_revision"], self.mutation["candidate_set"]["revision"],
                "fragment mutation revision"
            );
            assert_eq!(
                source["snapshot_id"], self.mutation["snapshot"]["id"],
                "fragment mutation snapshot"
            );
        }
        read
    }

    pub async fn read_overview(&self, client: &mut Mcp) -> ResolvedRead {
        self.read(client, "context_snapshot_and_knowledge", "overview")
            .await
    }

    pub async fn read_details(&self, client: &mut Mcp) -> ResolvedRead {
        let read = self.read(client, "draft", "details").await;
        assert!(
            read.value["draft"].is_object(),
            "stored draft must be present"
        );
        read
    }
}
