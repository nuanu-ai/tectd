use super::{ProgramReadProvenance, ResolvedProgramPage, keys, metadata, ready};
use crate::recovery_support::{Mcp, tool_payload};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

pub(super) async fn read(client: &mut Mcp, action: Value) -> ResolvedProgramPage {
    let base = ready(&action).clone();
    let initial = action["arguments"].clone();
    let mut arguments = initial.clone();
    let mut raw_exchanges = Vec::new();
    let mut maximum_mcp_bytes = 0;
    let mut bytes = Vec::new();
    let mut pin: Option<(String, usize)> = None;
    let mut continuation_limit = None;
    for pages in 1..=8192 {
        let raw = client
            .exchange("tools/call", json!({"name":"query", "arguments":arguments}))
            .await;
        assert!(raw.get("error").is_none());
        assert_ne!(raw["result"]["isError"], true);
        let size = serde_json::to_vec(&raw["result"]).unwrap().len();
        assert!(
            size <= 8192,
            "actual Program MCP result exceeds budget: {size}"
        );
        maximum_mcp_bytes = maximum_mcp_bytes.max(size);
        let page = tool_payload(&raw);
        raw_exchanges.push(raw);
        let (terminal_actions, terminal_recommended_action) = metadata(&page);
        if page["kind"] != "fragment" {
            assert!(bytes.is_empty() && pin.is_none() && pages == 1);
            return ResolvedProgramPage {
                value: page,
                provenance: ProgramReadProvenance {
                    initial_action: action,
                    initial_query: initial,
                    raw_exchanges,
                    representation_digest: None,
                    maximum_mcp_bytes,
                    pages,
                    terminal_actions,
                    terminal_recommended_action,
                    next_after_input: None,
                },
            };
        }
        assert_eq!(
            keys(&page),
            BTreeSet::from([
                "kind",
                "format",
                "encoding",
                "source",
                "representation_digest",
                "total_bytes",
                "offset_bytes",
                "returned_bytes",
                "text",
                "next_offset_bytes",
                "actions",
                "recommended_action"
            ])
        );
        assert_eq!(page["format"], "json");
        assert_eq!(page["encoding"], "utf-8");
        assert_eq!(
            page["source"],
            json!({"program_id":base["program_id"], "program_revision":base["program_revision"]})
        );
        let digest = page["representation_digest"].as_str().unwrap();
        assert!(
            digest.len() == 64
                && digest
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        );
        let total = usize::try_from(page["total_bytes"].as_u64().unwrap()).unwrap();
        assert!(total > 0 && total <= 8 * 1024 * 1024);
        match &pin {
            Some((expected_digest, expected_total)) => {
                assert_eq!(digest, expected_digest);
                assert_eq!(total, *expected_total);
            }
            None => {
                pin = Some((digest.into(), total));
            }
        }
        let offset = usize::try_from(page["offset_bytes"].as_u64().unwrap()).unwrap();
        assert_eq!(offset, bytes.len());
        let text = page["text"].as_str().unwrap();
        let count = usize::try_from(page["returned_bytes"].as_u64().unwrap()).unwrap();
        assert!(count > 0 && count <= 4096);
        assert_eq!(count, text.len());
        let end = offset.checked_add(count).unwrap();
        assert!(end <= total);
        bytes.extend_from_slice(text.as_bytes());
        if page["next_offset_bytes"].is_null() {
            assert_eq!(end, total);
            assert_eq!(format!("{:x}", Sha256::digest(&bytes)), digest);
            let value = serde_json::from_slice(&bytes).expect("original complete ProgramPage JSON");
            return ResolvedProgramPage {
                value,
                provenance: ProgramReadProvenance {
                    initial_action: action,
                    initial_query: initial,
                    raw_exchanges,
                    representation_digest: Some(digest.into()),
                    maximum_mcp_bytes,
                    pages,
                    terminal_actions,
                    terminal_recommended_action,
                    next_after_input: None,
                },
            };
        }
        assert_eq!(page["next_offset_bytes"].as_u64(), Some(end as u64));
        assert!(end < total);
        assert_eq!(terminal_actions.len(), 1, "unique actual byte continuation");
        assert_eq!(terminal_recommended_action, Some(0));
        let next = &terminal_actions[0];
        let params = ready(next);
        assert_eq!(
            keys(params),
            BTreeSet::from([
                "program_id",
                "program_revision",
                "after_input",
                "limit",
                "offset_bytes",
                "limit_bytes",
                "representation_digest"
            ])
        );
        for (key, original) in base.as_object().unwrap() {
            assert_eq!(params.get(key), Some(original));
        }
        assert_eq!(params["offset_bytes"].as_u64(), Some(end as u64));
        assert_eq!(
            params["representation_digest"],
            page["representation_digest"]
        );
        let limit = params["limit_bytes"].as_u64().unwrap();
        assert!(limit > 0 && limit <= 4096);
        assert_eq!(limit, 4096, "producer default initial fragment limit");
        if let Some(previous) = continuation_limit {
            assert_eq!(limit, previous);
        }
        continuation_limit = Some(limit);
        arguments = next["arguments"].clone();
    }
    panic!("Program fixture exceeds byte page budget");
}
