//! Stateless byte windows over the complete JSON representation of authorized reads.
use crate::responses;
use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tect_domain::{Error, RefusalCode, Result};

pub(crate) const READ_BUDGET: usize = 8192;
#[derive(Clone, Copy, Default)]
pub(crate) struct Window<'a> {
    pub offset_bytes: Option<u64>,
    pub limit_bytes: Option<u64>,
    pub representation_digest: Option<&'a str>,
}
impl Window<'_> {
    fn explicit(self) -> bool {
        self.offset_bytes.is_some()
            || self.limit_bytes.is_some()
            || self.representation_digest.is_some()
    }
}

/// `source` contains the immutable route pins. The continuation repeats them and
/// adds a representation digest; it never authorizes access on its own.
pub(crate) fn encode<T: Serialize>(
    value: &T,
    legacy_actions: Vec<Value>,
    capacity: usize,
    window: Window<'_>,
    source: Value,
    tool: &str,
    params: Value,
) -> Result<Value> {
    encode_with_continuation(
        value,
        legacy_actions,
        capacity,
        window,
        source,
        params,
        |params| responses::action(tool, params),
    )
}

/// Keep the byte representation and EOF behavior while allowing a bounded,
/// truthful continuation that requires caller input instead of echoing large arrays.
pub(crate) fn encode_with_continuation<T: Serialize>(
    value: &T,
    legacy_actions: Vec<Value>,
    capacity: usize,
    window: Window<'_>,
    source: Value,
    mut params: Value,
    continuation: impl Fn(Value) -> Result<Value>,
) -> Result<Value> {
    tect_domain::validate_pipeline_fragment(
        window.offset_bytes,
        window.limit_bytes,
        window.representation_digest,
    )?;
    let capacity = capacity.min(READ_BUDGET);
    let full = serde_json::to_value(value).map_err(|_| Error::TransportUnavailable)?;
    let bytes = serde_json::to_vec(&full).map_err(|_| Error::TransportUnavailable)?;
    let digest = format!("{:x}", Sha256::digest(&bytes));
    if window
        .representation_digest
        .is_some_and(|pin| pin != digest)
    {
        return Err(refusal(
            "representation_digest",
            "digest of the current authorized JSON representation",
            "representation changed",
            "restart_fragment_at_offset_zero",
        ));
    }
    let offset =
        usize::try_from(window.offset_bytes.unwrap_or(0)).map_err(|_| Error::InvalidArguments)?;
    let text = std::str::from_utf8(&bytes).map_err(|_| Error::InternalInvariant)?;
    if offset > bytes.len() || !text.is_char_boundary(offset) {
        return Err(refusal(
            "offset_bytes",
            "UTF-8 boundary within 0..=total_bytes",
            "invalid byte offset",
            "restart_fragment_at_offset_zero",
        ));
    }
    let full = responses::with_actions(full, legacy_actions.clone(), Some(0));
    if !window.explicit() && responses::encoded_len(&full)? <= capacity {
        return Ok(full);
    }
    params["representation_digest"] = json!(digest);
    params["limit_bytes"] = json!(window.limit_bytes.unwrap_or(4096));
    let page = |start: usize, end: usize| -> Result<Value> {
        let next = (end < bytes.len()).then_some(end);
        let actions = if let Some(next) = next {
            let mut next_params = params.clone();
            next_params["offset_bytes"] = json!(next);
            vec![continuation(next_params)?]
        } else {
            legacy_actions.clone()
        };
        let recommended = (!actions.is_empty()).then_some(0);
        Ok(responses::with_actions(
            json!({"kind":"fragment","format":"json","encoding":"utf-8",
            "source":source,"representation_digest":digest,"total_bytes":bytes.len(),
            "offset_bytes":start,"returned_bytes":end-start,"text":&text[start..end],
            "next_offset_bytes":next}),
            actions,
            recommended,
        ))
    };
    // Terminal actions must remain deliverable, including on an explicit empty EOF.
    // Never fall back to an EOF response that silently drops those actions.
    let empty_eof = page(bytes.len(), bytes.len())?;
    if responses::encoded_len(&empty_eof)? > capacity {
        return Err(Error::RequestTooLarge);
    }
    if offset == bytes.len() {
        return Ok(empty_eof);
    }
    let mut upper = offset
        .saturating_add(window.limit_bytes.unwrap_or(4096) as usize)
        .min(bytes.len());
    while !text.is_char_boundary(upper) {
        upper -= 1;
    }
    // Terminal and intermediate pages have distinct overhead. Test EOF separately.
    if upper == bytes.len() {
        let terminal = page(offset, upper)?;
        if responses::encoded_len(&terminal)? <= capacity {
            return Ok(terminal);
        }
    }
    let ends: Vec<usize> = (offset..=upper)
        .filter(|end| *end < bytes.len() && text.is_char_boundary(*end))
        .collect();
    let mut lo = 0;
    let mut hi = ends.len();
    let mut best = None;
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        let candidate = page(offset, ends[mid])?;
        if responses::encoded_len(&candidate)? <= capacity {
            best = Some((ends[mid], candidate));
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    match best {
        Some((end, value)) if end > offset => Ok(value),
        _ => Err(Error::RequestTooLarge),
    }
}
fn refusal(field: &'static str, expected: &str, actual: &str, next: &'static str) -> Error {
    Error::refused_at(
        RefusalCode::InputSchemaInvalid,
        "PIPELINE-JSON-FRAGMENT-REPRESENTATION",
        match field {
            "offset_bytes" => "arguments.params.offset_bytes",
            _ => "arguments.params.representation_digest",
        },
        expected,
        actual,
        next,
        field,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    fn encode_fixture(
        value: &Value,
        offset: Option<u64>,
        digest: Option<&str>,
        capacity: usize,
    ) -> Result<Value> {
        encode(
            value,
            vec![],
            capacity,
            Window {
                offset_bytes: offset,
                limit_bytes: None,
                representation_digest: digest,
            },
            json!({"run_id":"00000000-0000-0000-0000-000000000001","output_id":"00000000-0000-0000-0000-000000000002","digest":"body-pin"}),
            "slice_pipeline_context",
            json!({"run_id":"00000000-0000-0000-0000-000000000001","view":"output","output_id":"00000000-0000-0000-0000-000000000002","digest":"body-pin"}),
        )
    }
    #[test]
    fn complete_json_reassembles_under_real_envelope_budget() {
        let value = json!({"body":"\\\"\n🙂漢".repeat(7000),"artifacts":[{"body":"oversized".repeat(9000)}],"fields":{"large":"q".repeat(20000)}});
        let expected = serde_json::to_vec(&value).unwrap();
        let mut offset = None;
        let mut digest: Option<String> = None;
        let mut assembled = Vec::new();
        let mut max = 0;
        loop {
            let page = encode_fixture(&value, offset, digest.as_deref(), 8192).unwrap();
            assert_eq!(
                page,
                encode_fixture(&value, offset, digest.as_deref(), 8192).unwrap()
            );
            let size = responses::encoded_len(&page).unwrap();
            max = max.max(size);
            assert!(size <= 8192);
            assert_eq!(
                page["offset_bytes"].as_u64().unwrap() as usize,
                assembled.len()
            );
            assembled.extend_from_slice(page["text"].as_str().unwrap().as_bytes());
            digest = Some(page["representation_digest"].as_str().unwrap().into());
            let Some(next) = page["next_offset_bytes"].as_u64() else {
                break;
            };
            assert!(next > offset.unwrap_or(0));
            let params = &page["actions"][0]["arguments"]["params"];
            assert_eq!(params["offset_bytes"], next);
            assert_eq!(params["representation_digest"], digest.as_deref().unwrap());
            assert!(page["actions"][0].get("route_contract").is_none());
            offset = Some(next);
        }
        assert_eq!(assembled, expected);
        assert_eq!(digest.unwrap(), format!("{:x}", Sha256::digest(&expected)));
        println!("max_fixture_envelope_bytes={max}");
    }
    #[test]
    fn small_legacy_shape_and_explicit_final_empty_page() {
        let value = json!({"body":"small"});
        let legacy = encode_fixture(&value, None, None, 8192).unwrap();
        assert_eq!(legacy["body"], "small");
        assert!(legacy.get("kind").is_none());
        let bytes = serde_json::to_vec(&value).unwrap();
        let digest = format!("{:x}", Sha256::digest(&bytes));
        let final_page =
            encode_fixture(&value, Some(bytes.len() as u64), Some(&digest), 8192).unwrap();
        assert_eq!(final_page["text"], "");
        assert!(final_page["next_offset_bytes"].is_null());
    }
    #[test]
    fn invalid_windows_and_changed_representation_refuse_without_body() {
        let value = json!({"body":"🙂secret"});
        let bytes = serde_json::to_vec(&value).unwrap();
        let digest = format!("{:x}", Sha256::digest(&bytes));
        for (offset, pin) in [
            (Some(1), None),
            (Some(bytes.len() as u64 + 1), Some(digest.as_str())),
            (Some(11), Some(digest.as_str())),
            (
                Some(1),
                Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
            ),
        ] {
            let error = encode_fixture(&value, offset, pin, 8192).unwrap_err();
            assert!(error.refusal().is_some());
            assert!(!format!("{error:?}").contains("secret"));
        }
        assert_eq!(
            encode_fixture(&value, None, None, 1).unwrap_err(),
            Error::RequestTooLarge
        );
    }
    fn with_eof(
        value: &Value,
        actions: Vec<Value>,
        offset: u64,
        limit: u64,
        capacity: usize,
    ) -> Result<Value> {
        let bytes = serde_json::to_vec(value).unwrap();
        let digest = format!("{:x}", Sha256::digest(&bytes));
        encode(
            value,
            actions,
            capacity,
            Window {
                offset_bytes: Some(offset),
                limit_bytes: Some(limit),
                representation_digest: Some(&digest),
            },
            json!({"id":"source"}),
            "slice_pipeline_context",
            json!({"run_id":"00000000-0000-0000-0000-000000000001","view":"output","output_id":"00000000-0000-0000-0000-000000000002","digest":"body-pin"}),
        )
    }
    #[test]
    fn collection_cursor_is_visible_only_after_complete_json_and_empty_eof() {
        let value = json!({"body":"large\\\"🙂".repeat(10000)});
        let bytes = serde_json::to_vec(&value).unwrap();
        let next = json!({"kind":"ready_call","tool":"query","arguments":{"route":"program.get","params":{"program_id":"00000000-0000-0000-0000-000000000003","after_input":42,"limit":10}}});
        let actions = vec![next.clone()];
        let mut offset = 0;
        let mut assembled = Vec::new();
        loop {
            let page = with_eof(&value, actions.clone(), offset, 4096, 8192).unwrap();
            assert!(responses::encoded_len(&page).unwrap() <= 8192);
            assembled.extend_from_slice(page["text"].as_str().unwrap().as_bytes());
            if let Some(next_offset) = page["next_offset_bytes"].as_u64() {
                assert!(next_offset > offset);
                assert_ne!(page["actions"][0], next);
                assert_eq!(page["actions"].as_array().unwrap().len(), 1);
                offset = next_offset;
            } else {
                assert_eq!(page["actions"], json!(actions));
                assert_eq!(page["recommended_action"], 0);
                break;
            }
        }
        assert_eq!(assembled, bytes);
        let empty = with_eof(&value, actions.clone(), bytes.len() as u64, 4096, 8192).unwrap();
        assert_eq!(empty["text"], "");
        assert_eq!(empty["actions"], json!(actions));
    }
    #[test]
    fn terminal_and_nonterminal_overheads_are_fitted_independently() {
        let value = json!({"body":"abcdefgh"});
        let total = serde_json::to_vec(&value).unwrap().len() as u64;
        let actions = vec![json!({"cursor":"x".repeat(1800)})];
        let eof = with_eof(&value, actions.clone(), total, 4096, 8192).unwrap();
        let capacity = responses::encoded_len(&eof).unwrap();
        let prefix = with_eof(&value, actions.clone(), 0, 4096, capacity).unwrap();
        assert!(prefix["next_offset_bytes"].as_u64().unwrap() < total);
        assert_ne!(prefix["actions"], json!(actions));
        assert_eq!(
            with_eof(&value, actions.clone(), total, 4096, capacity - 1).unwrap_err(),
            Error::RequestTooLarge
        );
        assert_eq!(
            with_eof(&value, actions, total - 1, 4096, capacity).unwrap_err(),
            Error::RequestTooLarge
        );
        // With no EOF action, a terminal response can fit where any byte continuation cannot.
        let terminal = with_eof(&value, vec![], 0, 4096, 8192).unwrap();
        let capacity = responses::encoded_len(&terminal).unwrap();
        assert!(
            with_eof(&value, vec![], 0, 4096, capacity).unwrap()["next_offset_bytes"].is_null()
        );
        assert_eq!(
            with_eof(&value, vec![], 0, 1, capacity).unwrap_err(),
            Error::RequestTooLarge
        );
    }
    #[test]
    fn remaining_unicode_never_becomes_empty_progress_when_budget_is_too_small() {
        let value = json!({"body":"🙂"});
        let whole_character = with_eof(&value, vec![], 9, 4, 8192).unwrap();
        let capacity = responses::encoded_len(&whole_character).unwrap() - 1;
        assert_eq!(
            with_eof(&value, vec![], 9, 4, capacity).unwrap_err(),
            Error::RequestTooLarge
        );
    }
}
