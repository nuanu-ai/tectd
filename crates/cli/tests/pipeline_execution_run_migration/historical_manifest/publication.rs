//! One explicit public lifecycle read, following only its actual byte continuations.
use super::*;

pub(super) async fn origin(
    client: &mut Mcp,
    receipt: &KnowledgePublisherReceipt,
) -> KnowledgeChangeOrigin {
    let initial = json!({"route":"knowledge.lifecycle","params":{"change_id":receipt.change_id,"view":"current"}});
    let mut arguments = initial.clone();
    let mut bytes = Vec::new();
    let mut pinned: Option<(String, usize)> = None;
    let mut maximum_mcp_bytes = 0;
    let mut maximum_fragment_bytes = 0;
    for page_index in 0..8192 {
        let rpc = client
            .exchange("tools/call", json!({"name":"query","arguments":arguments}))
            .await;
        let outer_rpc_bytes = serde_json::to_vec(&rpc).unwrap().len();
        let actual_mcp_result_bytes = serde_json::to_vec(&rpc["result"]).unwrap().len();
        let page = recovery_support::tool_payload(&rpc);
        let fragment_offset = page
            .get("fragment")
            .and_then(|f| f.get("offset"))
            .and_then(Value::as_u64);
        maximum_mcp_bytes = maximum_mcp_bytes.max(actual_mcp_result_bytes);
        assert!(
            outer_rpc_bytes <= 8 * 1024 * 1024,
            "outer_rpc_bytes={outer_rpc_bytes}"
        );
        assert!(
            actual_mcp_result_bytes <= 8192,
            "outer_rpc_bytes={outer_rpc_bytes} actual_mcp_result_bytes={actual_mcp_result_bytes} page_index={page_index} fragment_offset={fragment_offset:?}"
        );
        assert_ne!(rpc["result"]["isError"], true);
        assert!(rpc.get("error").is_none());
        let Some(fragment) = page.get("fragment") else {
            assert!(bytes.is_empty(), "fragment stream changed representation");
            return verify(page, receipt);
        };
        assert_eq!(fragment["encoding"], "utf8");
        let offset = fragment["offset"].as_u64().unwrap() as usize;
        let count = fragment["byte_length"].as_u64().unwrap() as usize;
        let total = fragment["total_bytes"].as_u64().unwrap() as usize;
        let digest = fragment["snapshot_digest"].as_str().unwrap().to_owned();
        let text = fragment["text"].as_str().unwrap();
        assert_eq!(offset, bytes.len());
        assert_eq!(count, text.len());
        assert!(count <= 4096);
        maximum_fragment_bytes = maximum_fragment_bytes.max(count);
        assert!(total <= 8 * 1024 * 1024 && count > 0);
        assert!(offset + count <= total);
        if let Some(pin) = &pinned {
            assert_eq!(pin, &(digest.clone(), total));
        } else {
            pinned = Some((digest.clone(), total));
        }
        bytes.extend_from_slice(text.as_bytes());
        if bytes.len() == total {
            assert_eq!(page["actions"], json!([]));
            assert!(page.get("recommended_action").is_some_and(Value::is_null));
            assert_eq!(sha(&bytes), digest);
            eprintln!(
                "public_lifecycle_read total_bytes={total} pages={} max_mcp_bytes={maximum_mcp_bytes} max_fragment_bytes={maximum_fragment_bytes} sha256={digest}",
                page_index + 1
            );
            return verify(serde_json::from_slice(&bytes).unwrap(), receipt);
        }
        let actions = page["actions"].as_array().unwrap();
        assert_eq!(actions.len(), 1);
        assert_eq!(page["recommended_action"], 0);
        let next = &actions[0];
        assert_eq!(next["kind"], "ready_call");
        assert_eq!(next["tool"], "query");
        assert_eq!(action_name(next), Some("knowledge.lifecycle"));
        let params = action_params(next);
        assert_eq!(params["change_id"], json!(receipt.change_id));
        assert_eq!(params["view"], "current");
        assert_eq!(params["fragment"]["offset"], json!(bytes.len()));
        assert_eq!(params["fragment"]["snapshot_digest"], digest);
        let limit = params["fragment"]["limit"].as_u64().unwrap();
        assert!(limit > 0 && limit <= 4096);
        let mut expected = initial["params"].clone();
        expected["fragment"] = params["fragment"].clone();
        assert_eq!(params, &expected);
        assert_eq!(params["fragment"].as_object().unwrap().len(), 3);
        arguments = next["arguments"].clone();
    }
    panic!("public lifecycle read exceeded fixture page budget")
}

fn verify(value: Value, receipt: &KnowledgePublisherReceipt) -> KnowledgeChangeOrigin {
    let current = &value["current"];
    assert_eq!(current["change_id"], json!(receipt.change_id));
    assert_eq!(current["run"]["id"], json!(receipt.run_id));
    assert_eq!(
        current["publisher_receipt"],
        serde_json::to_value(receipt).unwrap()
    );
    serde_json::from_value(current["origin"].clone()).unwrap()
}

pub(super) fn assert_identity(
    publication: &Value,
    change_id: uuid::Uuid,
    operation_id: uuid::Uuid,
) {
    assert!(publication.get("change_id").is_some_and(Value::is_null));
    assert_eq!(publication["lifecycle_change_id"], json!(change_id));
    assert_eq!(publication["operation_id"], json!(operation_id));
}
