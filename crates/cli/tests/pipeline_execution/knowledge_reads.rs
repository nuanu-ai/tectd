//! Explicit authenticated knowledge reads; original responses are never rewritten.
#[path = "knowledge_reads/maintenance.rs"]
mod maintenance;
#[path = "knowledge_reads/phase_completion.rs"]
mod phase_completion;
use crate::recovery_support::{Mcp, tool_payload};
pub use phase_completion::phase_completion_action;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

pub struct ReadProvenance {
    pub initial_action: Option<Value>,
    pub initial_query: Value,
    pub raw_pages: Vec<Value>,
    pub representation_digest: Option<String>,
    pub maximum_mcp_bytes: usize,
}
pub struct ResolvedKnowledgeView {
    pub raw_response: Value,
    pub value: Value,
    pub provenance: Option<ReadProvenance>,
}
fn keys(value: &Value) -> BTreeSet<&str> {
    value
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect()
}
fn uuid(value: &Value) {
    uuid::Uuid::parse_str(value.as_str().unwrap()).unwrap();
}
fn metadata(value: &Value) {
    let actions = value["actions"].as_array().expect("actual actions array");
    let recommended = value
        .get("recommended_action")
        .expect("recommendation must be present");
    assert!(
        recommended.is_null()
            || recommended
                .as_u64()
                .is_some_and(|n| n < actions.len() as u64),
        "recommendation must index actual actions"
    );
}
fn inline_context(value: &Value) -> Option<&Value> {
    let contexts: Vec<_> = ["created", "advanced", "current", "replay"]
        .iter()
        .filter_map(|key| value.get(*key))
        .filter(|v| v.get("change_id").is_some() && v.get("run").is_some_and(Value::is_object))
        .collect();
    assert!(contexts.len() <= 1, "ambiguous actual context variants");
    contexts.first().copied()
}
fn compact(raw: &Value) {
    let outcome = raw["outcome"].as_str().expect("compact outcome required");
    assert!(raw.get("changed").is_some_and(Value::is_boolean));
    assert_eq!(raw["changed"], outcome != "replay");
    uuid(&raw["change_id"]);
    uuid(&raw["run_id"]);
    if raw.get("publisher_receipt_id").is_some() {
        uuid(&raw["publisher_receipt_id"]);
        assert!(matches!(outcome, "applied" | "replay" | "applied_erased"));
        if outcome != "applied_erased" {
            let digest = raw["publisher_receipt_digest"]
                .as_str()
                .expect("receipt digest required");
            assert!(digest.len() == 64 && digest.bytes().all(|b| b.is_ascii_hexdigit()));
            assert!(raw["workspace_generation"].as_i64().is_some_and(|n| n >= 0));
        }
    } else {
        assert!(matches!(outcome, "created" | "advanced" | "replay"));
        assert!(raw["run_revision"].as_i64().is_some_and(|n| n > 0));
        serde_json::from_value::<tect_domain::PipelineRunStatus>(raw["status"].clone()).unwrap();
        let phase = raw
            .get("current_phase_id")
            .expect("current phase pin must be present");
        let _: Option<tect_domain::KnowledgeChangePhaseId> =
            serde_json::from_value(phase.clone()).unwrap();
    }
}
fn selector(arguments: &Value) -> Value {
    assert_eq!(keys(arguments), BTreeSet::from(["route", "params"]));
    let mut params = arguments["params"].clone();
    if let Some(fragment) = params.as_object_mut().unwrap().remove("fragment") {
        assert_eq!(fragment["offset"], 0);
        assert!(
            fragment["limit"]
                .as_u64()
                .is_some_and(|n| n > 0 && n <= 4096)
        );
        assert_eq!(keys(&fragment), BTreeSet::from(["offset", "limit"]));
    }
    match arguments["route"].as_str().unwrap() {
        "knowledge.lifecycle" => {
            assert_eq!(keys(&params), BTreeSet::from(["change_id", "view"]));
            uuid(&params["change_id"]);
            assert_eq!(params["view"], "current");
        }
        "knowledge.unit" => {
            assert_eq!(keys(&params), BTreeSet::from(["unit_id", "revision"]));
            uuid(&params["unit_id"]);
            assert!(params["revision"].as_i64().is_some_and(|n| n > 0));
        }
        _ => panic!("unsupported explicit knowledge read"),
    }
    json!({"route":arguments["route"],"params":params})
}
fn verify(value: &Value, initial: &Value) {
    let params = &initial["params"];
    if initial["route"] == "knowledge.lifecycle" {
        assert_eq!(value["current"]["change_id"], params["change_id"]);
        uuid(&value["current"]["run"]["id"]);
        assert!(
            value["current"]["run"]["revision"]
                .as_i64()
                .is_some_and(|n| n > 0)
        );
    } else {
        let unit = value
            .get("document")
            .or_else(|| value.get("legacy_constraint"))
            .or_else(|| value.get("payload_erased"))
            .expect("actual unit response variant");
        assert_eq!(unit["unit_id"], params["unit_id"]);
        // Erased payloads deliberately have no document/revision to recover.
        if value.get("payload_erased").is_none() {
            assert_eq!(unit["revision"], params["revision"]);
        }
    }
    metadata(value);
}
async fn read(client: &mut Mcp, initial: Value, action: Option<Value>) -> ResolvedKnowledgeView {
    let source = selector(&initial);
    let mut arguments = initial.clone();
    let mut bytes = Vec::new();
    let mut pin = None;
    let mut raw_pages = Vec::new();
    let mut maximum_mcp_bytes = 0;
    for _ in 0..8192 {
        let raw = client
            .exchange("tools/call", json!({"name":"query","arguments":arguments}))
            .await;
        assert!(raw.get("error").is_none());
        assert_ne!(raw["result"]["isError"], true);
        let size = serde_json::to_vec(&raw["result"]).unwrap().len();
        assert!(
            size <= 8192,
            "actual MCP knowledge envelope exceeds budget: {size}"
        );
        maximum_mcp_bytes = maximum_mcp_bytes.max(size);
        let page = tool_payload(&raw);
        raw_pages.push(raw);
        let finish = |value: Value, digest: Option<String>, raw_pages: Vec<Value>| {
            verify(&value, &source);
            ResolvedKnowledgeView {
                raw_response: tool_payload(&raw_pages[0]),
                value,
                provenance: Some(ReadProvenance {
                    initial_action: action.clone(),
                    initial_query: initial.clone(),
                    raw_pages,
                    representation_digest: digest,
                    maximum_mcp_bytes,
                }),
            }
        };
        let Some(fragment) = page.get("fragment") else {
            assert!(
                bytes.is_empty() && raw_pages.len() == 1,
                "fragment changed representation"
            );
            return finish(page.clone(), None, raw_pages);
        };
        assert_eq!(
            keys(fragment),
            BTreeSet::from([
                "encoding",
                "snapshot_digest",
                "offset",
                "byte_length",
                "total_bytes",
                "text"
            ])
        );
        assert_eq!(fragment["encoding"], "utf8");
        let offset = fragment["offset"].as_u64().unwrap() as usize;
        let count = fragment["byte_length"].as_u64().unwrap() as usize;
        let total = fragment["total_bytes"].as_u64().unwrap() as usize;
        let digest = fragment["snapshot_digest"].as_str().unwrap().to_owned();
        let text = fragment["text"].as_str().unwrap();
        assert!(digest.len() == 64 && digest.bytes().all(|b| b.is_ascii_hexdigit()));
        assert_eq!(offset, bytes.len());
        assert_eq!(count, text.len());
        assert!(count > 0 && count <= 4096 && total <= 8 * 1024 * 1024);
        assert!(offset.checked_add(count).is_some_and(|end| end <= total));
        if let Some(previous) = &pin {
            assert_eq!(previous, &(digest.clone(), total));
        } else {
            pin = Some((digest.clone(), total));
        }
        bytes.extend_from_slice(text.as_bytes());
        if bytes.len() == total {
            assert_eq!(page["actions"], json!([]));
            assert!(page.get("recommended_action").is_some_and(Value::is_null));
            let actual = Sha256::digest(&bytes)
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>();
            assert_eq!(actual, digest);
            return finish(
                serde_json::from_slice(&bytes).unwrap(),
                Some(digest),
                raw_pages,
            );
        }
        let actions = page["actions"].as_array().unwrap();
        assert_eq!(actions.len(), 1);
        assert_eq!(page["recommended_action"], 0);
        let next = &actions[0];
        assert_eq!(keys(next), BTreeSet::from(["kind", "tool", "arguments"]));
        assert_eq!(next["kind"], "ready_call");
        assert_eq!(next["tool"], "query");
        assert_eq!(
            keys(&next["arguments"]),
            BTreeSet::from(["route", "params"])
        );
        assert_eq!(next["arguments"]["route"], source["route"]);
        let mut next_params = next["arguments"]["params"].clone();
        let cursor = next_params
            .as_object_mut()
            .unwrap()
            .remove("fragment")
            .unwrap();
        assert_eq!(next_params, source["params"]);
        assert_eq!(
            keys(&cursor),
            BTreeSet::from(["snapshot_digest", "offset", "limit"])
        );
        assert_eq!(cursor["snapshot_digest"], digest);
        assert_eq!(cursor["offset"], json!(bytes.len()));
        assert!(cursor["limit"].as_u64().is_some_and(|n| n > 0 && n <= 4096));
        if let Some(prior) = arguments["params"].get("fragment") {
            assert_eq!(cursor["limit"], prior["limit"]);
        } else {
            assert_eq!(cursor["limit"], 4096);
        }
        arguments = next["arguments"].clone();
    }
    panic!("explicit knowledge read exceeded finite fixture page budget")
}
pub async fn read_current(client: &mut Mcp, change_id: &Value) -> ResolvedKnowledgeView {
    read(
        client,
        json!({"route":"knowledge.lifecycle","params":{"change_id":change_id,"view":"current"}}),
        None,
    )
    .await
}
pub async fn read_unit(
    client: &mut Mcp,
    unit_id: &Value,
    revision: &Value,
) -> ResolvedKnowledgeView {
    read(
        client,
        json!({"route":"knowledge.unit","params":{"unit_id":unit_id,"revision":revision}}),
        None,
    )
    .await
}
pub async fn resolve_current(client: &mut Mcp, raw: Value) -> ResolvedKnowledgeView {
    metadata(&raw);
    if maintenance::recognizes(&raw) {
        return maintenance::resolve(client, raw).await;
    }
    if let Some(context) = inline_context(&raw) {
        uuid(&context["change_id"]);
        uuid(&context["run"]["id"]);
        assert!(context["run"]["revision"].as_i64().is_some_and(|n| n > 0));
        return ResolvedKnowledgeView {
            value: raw.clone(),
            raw_response: raw,
            provenance: None,
        };
    }
    compact(&raw);
    let actions = raw["actions"].as_array().unwrap();
    let candidates: Vec<_> = actions
        .iter()
        .filter(|a| {
            a["kind"] == "ready_call"
                && a["tool"] == "query"
                && a["arguments"]["route"] == "knowledge.lifecycle"
                && a["arguments"]["params"]["view"] == "current"
        })
        .collect();
    assert_eq!(
        candidates.len(),
        1,
        "unique actual Current navigation required"
    );
    let action = (*candidates[0]).clone();
    assert_eq!(keys(&action), BTreeSet::from(["kind", "tool", "arguments"]));
    assert_eq!(action["arguments"]["params"]["change_id"], raw["change_id"]);
    let mut resolved = read(client, action["arguments"].clone(), Some(action)).await;
    let current = &resolved.value["current"];
    for (outer, inner) in [
        ("change_id", "change_id"),
        ("run_id", "id"),
        ("run_revision", "revision"),
        ("status", "status"),
        ("current_phase_id", "current_phase_id"),
    ] {
        if let Some(expected) = raw.get(outer) {
            let actual = if outer == "change_id" {
                &current[inner]
            } else {
                &current["run"][inner]
            };
            assert_eq!(actual, expected, "compact mutation pin drift: {outer}");
        }
    }
    resolved.raw_response = raw;
    resolved
}
pub fn producer_action<'a>(value: &'a Value, route: &str) -> &'a Value {
    metadata(value);
    let actions: Vec<_> = value["actions"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|a| {
            a["kind"] == "ready_call" && a["tool"] == "command" && a["arguments"]["route"] == route
        })
        .collect();
    assert_eq!(
        actions.len(),
        1,
        "unique expected knowledge producer action required: {route}"
    );
    let action = actions[0];
    let mut expected = BTreeSet::from(["kind", "tool", "arguments"]);
    if action.get("context_input").is_some() {
        expected.insert("context_input");
    }
    assert_eq!(keys(action), expected);
    action
}

pub struct ResolvedKnowledgeReceipt {
    pub raw_response: Value,
    pub receipt: Value,
    pub current_read: Option<ReadProvenance>,
}
pub async fn resolve_commit_receipt(client: &mut Mcp, raw: Value) -> ResolvedKnowledgeReceipt {
    metadata(&raw);
    if let Some(receipt) = ["applied", "replay", "applied_erased"]
        .iter()
        .find_map(|key| raw.get(*key))
    {
        assert!(receipt.is_object());
        return ResolvedKnowledgeReceipt {
            receipt: receipt.clone(),
            raw_response: raw,
            current_read: None,
        };
    }
    compact(&raw);
    assert!(matches!(
        raw["outcome"].as_str(),
        Some("applied" | "replay" | "applied_erased")
    ));
    let resolved = resolve_current(client, raw.clone()).await;
    let field = if raw["outcome"] == "applied_erased" {
        "erased_publisher_receipt"
    } else {
        "publisher_receipt"
    };
    let receipt = resolved.value["current"]
        .get(field)
        .filter(|v| v.is_object())
        .expect("actual current receipt unavailable; no erased content reconstruction")
        .clone();
    for (outer, inner) in [
        ("publisher_receipt_id", "id"),
        ("change_id", "change_id"),
        ("run_id", "run_id"),
        ("publisher_receipt_digest", "digest"),
        ("workspace_generation", "workspace_generation"),
    ] {
        if let Some(pin) = raw.get(outer) {
            assert_eq!(
                &receipt[inner], pin,
                "compact commit receipt pin drift: {outer}"
            );
        }
    }
    ResolvedKnowledgeReceipt {
        raw_response: raw,
        receipt,
        current_read: resolved.provenance,
    }
}

pub fn context(value: &Value) -> &Value {
    inline_context(value)
        .or_else(|| maintenance::inline_context(value))
        .expect("actual lifecycle context or documented maintenance begin context required")
}
