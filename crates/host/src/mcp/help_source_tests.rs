use super::*;
use sha2::{Digest, Sha256};

#[test]
fn public_help_route_fragments_keep_logical_source_through_full_mcp() {
    let initial = json!({"mode":"describe","tool":"query","route":"knowledge.search"});
    let full = crate::api::help(crate::api::parse_help(initial.clone()).unwrap()).unwrap();
    let expected = serde_json::to_vec(&full).unwrap();
    let digest = format!("{:x}", Sha256::digest(&expected));
    let source = json!({"tool":"help","selectors":initial});
    let id = json!(50);
    let overhead = serde_json::to_vec(&success_response(id.clone(), Value::Null))
        .unwrap()
        .len()
        - 4;
    let capacity = MAX_FRAME_BYTES - overhead;
    let mut arguments = initial.clone();
    let mut assembled = Vec::new();
    let mut pages = 0;
    let mut maximum = 0;
    loop {
        crate::api::decode_public_call("help", arguments.clone()).unwrap();
        let page = crate::planning_read::help(
            crate::api::parse_help(arguments.clone()).unwrap(),
            capacity,
        )
        .unwrap();
        let response = success_response(id.clone(), successful_tool_result(page.clone()));
        let wire = serde_json::to_vec(&response).unwrap();
        maximum = maximum.max(wire.len());
        assert!(wire.len() <= MAX_FRAME_BYTES);
        let actual: Value =
            serde_json::from_str(response["result"]["content"][1]["text"].as_str().unwrap())
                .unwrap();
        assert_eq!(actual, page);
        assert_eq!(page["kind"], "fragment");
        assert_eq!(page["format"], "json");
        assert_eq!(page["encoding"], "utf-8");
        assert_eq!(page["source"], source);
        assert_eq!(page["representation_digest"], digest);
        assert_eq!(
            page["total_bytes"].as_u64().unwrap() as usize,
            expected.len()
        );
        assert_eq!(
            page["offset_bytes"].as_u64().unwrap() as usize,
            assembled.len()
        );
        let text = page["text"].as_str().unwrap().as_bytes();
        assert_eq!(
            page["returned_bytes"].as_u64().unwrap() as usize,
            text.len()
        );
        assert!(!text.is_empty());
        assert!(text.len() <= arguments["limit_bytes"].as_u64().unwrap_or(4096) as usize);
        assembled.extend_from_slice(text);
        pages += 1;
        assert!(pages < 8192);
        let actions = page["actions"].as_array().unwrap();
        if page["next_offset_bytes"].is_null() {
            assert!(actions.is_empty());
            assert!(page["recommended_action"].is_null());
            break;
        }
        assert_eq!(
            page["next_offset_bytes"].as_u64().unwrap() as usize,
            assembled.len()
        );
        assert_eq!(actions.len(), 1);
        assert_eq!(page["recommended_action"], 0);
        let action = &actions[0];
        assert_eq!(action["tool"], "help");
        arguments = action["arguments"].clone();
        crate::api::decode_public_call("help", arguments.clone()).unwrap();
        for key in ["mode", "tool", "route"] {
            assert_eq!(arguments[key], initial[key]);
        }
        assert_eq!(
            arguments["offset_bytes"].as_u64().unwrap() as usize,
            assembled.len()
        );
        assert_eq!(arguments["representation_digest"], digest);
        assert_eq!(arguments["limit_bytes"], 4096);
    }
    assert!(pages > 1);
    assert_eq!(assembled, expected);
    assert_eq!(format!("{:x}", Sha256::digest(&assembled)), digest);
    assert_eq!(serde_json::from_slice::<Value>(&assembled).unwrap(), full);
    let mut changed = initial;
    changed["representation_digest"] = json!("0".repeat(64));
    changed["offset_bytes"] = json!(0);
    assert!(
        crate::planning_read::help(crate::api::parse_help(changed).unwrap(), capacity).is_err()
    );
    println!(
        "help[knowledge.search] pages={pages} total_bytes={} full_mcp_max_bytes={maximum}",
        assembled.len()
    );
}
