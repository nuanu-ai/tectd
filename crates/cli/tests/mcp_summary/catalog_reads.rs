//! Exact catalogue reader; raw Bridge responses remain separate from assembled DTOs.
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, ops::AsyncFnMut};

const ROUTE: &str = "slice.pipelines";
const MAX_BYTES: usize = 8 * 1024 * 1024;
const MAX_PAGES: usize = 8192;

pub struct Read {
    pub value: Value,
    pub initial_arguments: Value,
    pub source: Option<Value>,
    pub digest: Option<String>,
    pub pages: usize,
    pub maximum_envelope_bytes: usize,
}

pub async fn read(mut raw_call: impl AsyncFnMut(Value) -> Value, initial_params: Value) -> Read {
    let arguments = json!({"route":ROUTE,"params":initial_params});
    assert_eq!(arguments["route"], ROUTE);
    let initial = arguments["params"]
        .as_object()
        .expect("catalogue parameters");
    assert!(initial.keys().all(|key| key == "view"));
    let view = match initial.get("view") {
        None => "full",
        Some(value) => value
            .as_str()
            .expect("explicit catalogue view must be a string"),
    };
    assert!(matches!(view, "full" | "summary"));
    let expected_source = json!({"tool":"query","route":ROUTE,"view":view});
    let mut current = arguments.clone();
    let mut bytes = Vec::new();
    let mut source = None;
    let mut digest = None;
    let mut total = None;
    let mut fragment_keys = None;
    let mut limit = None;
    let mut maximum_envelope_bytes = 0;
    for pages in 1..=MAX_PAGES {
        let raw = raw_call(current.clone()).await;
        assert_ne!(raw["result"]["isError"], true, "{raw}");
        let envelope = serde_json::to_vec(&raw["result"]).unwrap().len();
        assert!(envelope <= 8192, "raw MCP tool envelope exceeded budget");
        maximum_envelope_bytes = maximum_envelope_bytes.max(envelope);
        assert!(raw.get("error").is_none(), "{raw}");
        let page: Value = serde_json::from_str(
            raw["result"]["content"][1]["text"]
                .as_str()
                .expect("actual JSON payload"),
        )
        .unwrap();
        if page["kind"] != "fragment" {
            assert_eq!(pages, 1, "fragment changed representation");
            assert!(bytes.is_empty());
            assert!(page["actions"].as_array().unwrap().is_empty());
            assert!(page["recommended_action"].is_null());
            return Read {
                value: page,
                initial_arguments: arguments,
                source,
                digest,
                pages,
                maximum_envelope_bytes,
            };
        }
        assert_eq!(page["format"], "json");
        assert_eq!(page["encoding"], "utf-8");
        assert_eq!(page["source"], expected_source, "exact catalogue source");
        let keys = page
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect::<BTreeSet<_>>();
        let page_digest = page["representation_digest"].as_str().expect("digest");
        assert!(
            page_digest.len() == 64
                && page_digest
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        );
        let page_total = usize::try_from(page["total_bytes"].as_u64().unwrap()).unwrap();
        assert!(page_total <= MAX_BYTES, "catalogue fixture byte bound");
        if pages == 1 {
            fragment_keys = Some(keys);
            source = Some(page["source"].clone());
            digest = Some(page_digest.to_owned());
            total = Some(page_total);
        } else {
            assert_eq!(
                fragment_keys.as_ref(),
                Some(&keys),
                "fragment keyset changed"
            );
            assert_eq!(digest.as_deref(), Some(page_digest), "digest changed");
            assert_eq!(total, Some(page_total), "total changed");
        }
        let offset = usize::try_from(page["offset_bytes"].as_u64().unwrap()).unwrap();
        assert_eq!(offset, bytes.len(), "noncontiguous fragment");
        let text = page["text"].as_str().expect("UTF-8 fragment");
        let returned = usize::try_from(page["returned_bytes"].as_u64().unwrap()).unwrap();
        assert_eq!(returned, text.len());
        assert!(returned <= 4096);
        if let Some(limit) = limit {
            assert!(returned <= limit);
        }
        let end = offset.checked_add(returned).unwrap();
        assert!(end <= page_total);
        bytes.extend_from_slice(text.as_bytes());
        let actions = page["actions"].as_array().expect("actual actions");
        if page["next_offset_bytes"].is_null() {
            assert_eq!(end, page_total, "premature EOF");
            assert!(actions.is_empty(), "catalogue EOF has no semantic actions");
            assert!(page["recommended_action"].is_null());
            assert_eq!(format!("{:x}", Sha256::digest(&bytes)), page_digest);
            let value: Value = serde_json::from_slice(&bytes).expect("complete catalogue JSON");
            assert!(value.is_object());
            assert!(
                value.get("actions").is_none(),
                "original DTO gains no transport metadata"
            );
            return Read {
                value,
                initial_arguments: arguments,
                source,
                digest,
                pages,
                maximum_envelope_bytes,
            };
        }
        assert!(returned > 0, "fragment must advance");
        assert_eq!(page["next_offset_bytes"].as_u64(), Some(end as u64));
        assert_eq!(actions.len(), 1, "unique actual continuation");
        assert_eq!(page["recommended_action"], 0);
        let next = &actions[0];
        assert_eq!(next["kind"], "ready_call");
        assert_eq!(next["tool"], "query");
        assert_eq!(next["arguments"]["route"], ROUTE);
        let params = next["arguments"]["params"].as_object().unwrap();
        assert_eq!(
            params.keys().map(String::as_str).collect::<BTreeSet<_>>(),
            BTreeSet::from([
                "view",
                "offset_bytes",
                "limit_bytes",
                "representation_digest"
            ])
        );
        assert_eq!(params["view"], view);
        assert_eq!(params["offset_bytes"].as_u64(), Some(end as u64));
        assert_eq!(
            params["representation_digest"],
            page["representation_digest"]
        );
        let next_limit = usize::try_from(params["limit_bytes"].as_u64().unwrap()).unwrap();
        assert!((1..=4096).contains(&next_limit));
        if let Some(limit) = limit {
            assert_eq!(limit, next_limit, "window limit changed");
        } else {
            limit = Some(next_limit);
        }
        current = next["arguments"].clone();
    }
    panic!("catalogue fixture page bound exceeded");
}
