use super::*;
use crate::slice_tools::{PipelineView, SliceInvocation};
use sha2::{Digest, Sha256};

fn read(params: Value, capacity: usize) -> tect_domain::Result<Value> {
    let call = crate::api::decode_public_call(
        "query",
        json!({"route":"slice.pipelines","params":params}),
    )?;
    assert_eq!(call.name, "slice_pipelines");
    match crate::slice_tools::parse(call.name, call.arguments)? {
        SliceInvocation::Pipelines(view) => crate::slice_dispatch::pipelines_read(
            view,
            &crate::planning_read::Window::default(),
            capacity,
        ),
        SliceInvocation::Window {
            request, window, ..
        } => match *request {
            SliceInvocation::Pipelines(view) => {
                crate::slice_dispatch::pipelines_read(view, &window, capacity)
            }
            _ => panic!("catalogue window must keep its logical route"),
        },
        _ => panic!("catalogue must remain a read"),
    }
}

#[test]
fn public_catalogue_reads_preserve_full_and_summary_through_bounded_mcp() {
    let id = json!(51);
    let overhead = serde_json::to_vec(&success_response(id.clone(), Value::Null))
        .unwrap()
        .len()
        - 4;
    let capacity = MAX_FRAME_BYTES - overhead;
    assert_eq!(
        read(json!({}), capacity).unwrap(),
        read(json!({"view":"full"}), capacity).unwrap()
    );
    for (view, expected, forced) in [
        ("full", crate::slice_pipeline_catalog::value(), false),
        (
            "summary",
            crate::slice_pipeline_catalog::summary_value(),
            false,
        ),
        (
            "summary",
            crate::slice_pipeline_catalog::summary_value(),
            true,
        ),
    ] {
        assert_eq!(expected["pipelines"].as_array().unwrap().len(), 9);
        if view == "full" {
            assert!(
                !expected["knowledge_change_entry"]["definition"]["phases"]
                    .as_array()
                    .unwrap()
                    .is_empty()
            );
            assert_eq!(
                expected["promotion_method"]["body"],
                include_str!("../../knowledge-methods/promotion-slice.md")
            );
        }
        let bytes = serde_json::to_vec(&expected).unwrap();
        let digest = format!("{:x}", Sha256::digest(&bytes));
        let source = json!({"tool":"query","route":"slice.pipelines","view":view});
        let mut params = json!({"view":view});
        if forced {
            params["limit_bytes"] = json!(128);
        }
        let mut assembled = Vec::new();
        let mut pages = 0;
        let mut maximum = 0;
        loop {
            let page = read(params.clone(), capacity).unwrap();
            let response = success_response(id.clone(), successful_tool_result(page.clone()));
            let wire = serde_json::to_vec(&response).unwrap();
            maximum = maximum.max(wire.len());
            assert!(wire.len() <= MAX_FRAME_BYTES);
            let actual: Value =
                serde_json::from_str(response["result"]["content"][1]["text"].as_str().unwrap())
                    .unwrap();
            assert_eq!(actual, page);
            pages += 1;
            assert!(pages < 8192);
            if page["kind"] != "fragment" {
                assert!(!forced);
                let mut original = page.clone();
                original.as_object_mut().unwrap().remove("actions");
                original
                    .as_object_mut()
                    .unwrap()
                    .remove("recommended_action");
                assert_eq!(original, expected);
                assert_eq!(page["actions"], json!([]));
                assert!(page["recommended_action"].is_null());
                break;
            }
            assert_eq!(page["source"], source);
            assert_eq!(page["representation_digest"], digest);
            assert_eq!(page["format"], "json");
            assert_eq!(page["encoding"], "utf-8");
            assert_eq!(page["total_bytes"].as_u64().unwrap() as usize, bytes.len());
            assert_eq!(
                page["offset_bytes"].as_u64().unwrap() as usize,
                assembled.len()
            );
            let text = page["text"].as_str().unwrap().as_bytes();
            assert!(!text.is_empty());
            assert_eq!(
                page["returned_bytes"].as_u64().unwrap() as usize,
                text.len()
            );
            assert!(text.len() <= params["limit_bytes"].as_u64().unwrap_or(4096) as usize);
            assembled.extend_from_slice(text);
            let actions = page["actions"].as_array().unwrap();
            if page["next_offset_bytes"].is_null() {
                assert!(actions.is_empty());
                assert!(page["recommended_action"].is_null());
                assert_eq!(assembled, bytes);
                assert_eq!(format!("{:x}", Sha256::digest(&assembled)), digest);
                assert_eq!(
                    serde_json::from_slice::<Value>(&assembled).unwrap(),
                    expected
                );
                break;
            }
            assert_eq!(
                page["next_offset_bytes"].as_u64().unwrap() as usize,
                assembled.len()
            );
            assert_eq!(page["recommended_action"], 0);
            assert_eq!(actions.len(), 1);
            let action = &actions[0];
            assert_eq!(action["tool"], "query");
            assert_eq!(action["arguments"]["route"], "slice.pipelines");
            crate::api::decode_public_call("query", action["arguments"].clone()).unwrap();
            params = action["arguments"]["params"].clone();
            assert_eq!(params["view"], view);
            assert_eq!(
                params["offset_bytes"].as_u64().unwrap() as usize,
                assembled.len()
            );
            assert_eq!(params["representation_digest"], digest);
            assert_eq!(params["limit_bytes"], if forced { 128 } else { 4096 });
        }
        println!(
            "catalogue[{view}] forced={forced} pages={pages} original_bytes={} max_full_mcp={maximum}",
            bytes.len()
        );
    }
}

#[test]
fn public_catalogue_windows_reject_invalid_selectors_and_pins() {
    for params in [
        json!({"extra":true}),
        json!({"view":null}),
        json!({"view":"unknown"}),
        json!({"offset_bytes":null}),
        json!({"limit_bytes":null}),
        json!({"representation_digest":null}),
        json!({"offset_bytes":-1}),
        json!({"limit_bytes":0}),
        json!({"limit_bytes":4097}),
        json!({"offset_bytes":1}),
        json!({"representation_digest":"bad"}),
        json!({"view":"full","limit_bytes":1,"extra":true}),
    ] {
        assert!(read(params, 8192).is_err());
    }
    assert!(
        read(
            json!({"view":"full","offset_bytes":0,"representation_digest":"0".repeat(64)}),
            8192
        )
        .is_err()
    );
    assert!(matches!(
        crate::slice_tools::parse("slice_pipelines", json!({})),
        Ok(SliceInvocation::Pipelines(PipelineView::Full))
    ));
}
