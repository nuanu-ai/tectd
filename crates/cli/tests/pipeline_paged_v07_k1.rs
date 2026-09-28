#[path = "pipeline_execution/knowledge_lifecycle_support.rs"]
#[allow(dead_code)]
mod knowledge_lifecycle_support;
#[path = "pipeline_execution/lifecycle_support.rs"]
#[allow(dead_code)]
mod lifecycle_support;
#[allow(dead_code)]
mod recovery_support;
#[path = "native_planning/support.rs"]
#[allow(dead_code)]
mod support;

use knowledge_lifecycle_support::commit_create;
use recovery_support::{Daemon, Mcp, host_file, private_temp, tagged_url};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use std::collections::BTreeMap;
use support::{open_slice, ready_source_candidate, repository, review, route, route_error, save};
use tect_postgres::admin;
use uuid::Uuid;

fn document(index: usize) -> Value {
    let mut fixture: Value = serde_json::from_str(include_str!(
        "../../postgres/src/knowledge_lifecycle/rdf/fixtures/general-constraint.json"
    ))
    .unwrap();
    let doc = &mut fixture["document"];
    doc["title"] = json!(format!("Paged K1 constraint {index}"));
    doc["canonical_text"] = json!(format!(
        "PAGED-K1-SECRET-{index}: exact pinned source {}",
        "x".repeat(12_000)
    ));
    doc["sources"][0]["snapshot"]["uri"] = json!(format!("urn:tect:paged-k1:source:{index}"));
    doc["sources"][0]["snapshot"]["text"] = json!(format!("Paged K1 source {index}"));
    doc.clone()
}

fn page_params(context: &Value, cursor: Option<&str>) -> Value {
    let mut params = json!({"run_id":context["run"]["id"],
        "manifest_id":context["knowledge_resources"]["id"],
        "digest":context["knowledge_resources"]["digest"],"byte_budget":8192});
    if let Some(cursor) = cursor {
        params["cursor"] = json!(cursor);
    }
    params
}

fn decode_base64url(data: &str) -> Vec<u8> {
    assert_ne!(data.len() % 4, 1);
    let mut decoded = Vec::new();
    let mut bits = 0_u32;
    let mut pending = 0_u32;
    for byte in data.bytes() {
        let value = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'-' => 62,
            b'_' => 63,
            _ => panic!("invalid unpadded base64url fragment"),
        };
        bits = (bits << 6) | u32::from(value);
        pending += 6;
        if pending >= 8 {
            pending -= 8;
            decoded.push((bits >> pending) as u8);
            bits &= (1 << pending) - 1;
        }
    }
    assert!(matches!(pending, 0 | 2 | 4) && bits == 0);
    decoded
}

#[test]
fn base64url_fragment_decoder_preserves_bytes() {
    assert_eq!(decode_base64url("AAH-_w"), [0, 1, 254, 255]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn paged_manifest_and_v07_k1_derive_exact_backend_knowledge_binding() {
    if std::env::var("TECT_TEST_DK2").as_deref() != Ok("1") {
        return;
    }
    let admin_url = std::env::var("TECT_TEST_ADMIN_URL").expect("dedicated DK admin URL required");
    let runtime_url =
        std::env::var("TECT_TEST_RUNTIME_URL").expect("dedicated DK runtime URL required");
    let role = std::env::var("TECT_TEST_RUNTIME_ROLE").expect("dedicated DK runtime role required");
    let pool = PgPool::connect(&admin_url).await.unwrap();
    admin::migrate(&pool, &role).await.unwrap();
    tect_postgres::enable_durable_knowledge(&pool, &role)
        .await
        .unwrap();

    let temp = private_temp();
    let root = temp.path().canonicalize().unwrap();
    let repo = root.join("source");
    repository(&repo);
    let socket = root.join("paged-v07-k1.sock");
    let runtime = tagged_url(&runtime_url, &format!("paged-v07-k1-{}", Uuid::new_v4()));
    let _daemon = Daemon::start(&runtime, socket.clone()).await;
    let enrollment = admin::enroll_host(&pool, None, vec![root.to_string_lossy().into_owned()])
        .await
        .unwrap();
    let config = root.join("host.json");
    host_file(&config, &enrollment.auth);
    let key = format!("paged-v07-k1-{}", Uuid::new_v4());
    let mut client = Mcp::start(&socket, &config, &Uuid::new_v4().to_string(), &key).await;
    let (source, candidate) = ready_source_candidate(&mut client, &repo).await;
    let opened = route(
        &mut client,
        "command",
        "scope.open",
        json!({"request_id":Uuid::new_v4(),
        "candidate_set_id":source["candidate_set"]["id"],
        "candidate_set_revision":source["candidate_set"]["revision"],
        "candidate_snapshot_id":source["snapshot"]["id"],
        "candidate_id":candidate["id"],"candidate_revision":candidate["revision"]}),
    )
    .await;
    let saved = save(
        &mut client,
        &opened["created"]["planning"],
        lifecycle_support::lightweight_draft(),
    )
    .await;
    let reviewed = review(&mut client, &saved).await;
    let slice = route(
        &mut client,
        "command",
        "slice.open",
        open_slice(&reviewed, &reviewed["draft"]["nodes"][0], Uuid::new_v4()),
    )
    .await;
    let first = commit_create(&mut client, document(0)).await;
    let second = commit_create(&mut client, document(1)).await;
    let units =
        [first, second].map(|item| item.receipt["applied_operations"][0]["unit_id"].clone());

    let begun = route(
        &mut client,
        "command",
        "slice.pipeline.begin",
        json!({
        "request_id":Uuid::new_v4(),"scope_id":reviewed["scope"]["id"],
        "slice_id":slice["created"]["id"],"slice_revision":slice["created"]["revision"],
        "definition_version":"0.7.0-native.k1k5",
        "qualification_reason":"Verify paged DK2 and backend-derived K1 proof."}),
    )
    .await;
    let context = &begun["created"];
    let manifest = &context["knowledge_resources"];
    assert_eq!(context["run"]["current_phase_id"], "K1");
    assert_eq!(manifest["contract_version"], "dk-2-paged");
    assert_eq!(manifest["resource_count"], 2);
    assert!(manifest.get("selected").is_none());

    let mut cursor = None;
    let mut seen = Vec::new();
    let mut split_bytes = BTreeMap::<String, Vec<u8>>::new();
    let mut verified_split = false;
    let mut first_cursor = None;
    let mut complete = false;
    for page_index in 0..64 {
        let page = route(
            &mut client,
            "query",
            "slice.pipeline.knowledge_page",
            page_params(context, cursor.as_deref()),
        )
        .await;
        assert_eq!(page["manifest_digest"], manifest["digest"]);
        for item in page["resources"].as_array().unwrap() {
            seen.push(item["resource"]["unit_id"].clone());
        }
        if page["fragment"].is_object() {
            let fragment = &page["fragment"];
            let pin = &fragment["pin"];
            let unit_id = pin["unit_id"].as_str().unwrap();
            seen.push(pin["unit_id"].clone());
            let bytes = split_bytes.entry(unit_id.to_owned()).or_default();
            assert_eq!(
                bytes.len(),
                fragment["byte_offset"].as_u64().unwrap() as usize
            );
            bytes.extend(decode_base64url(fragment["data"].as_str().unwrap()));
            let total = fragment["total_bytes"].as_u64().unwrap() as usize;
            assert!(bytes.len() <= total);
            if bytes.len() == total {
                let actual_digest = format!("{:x}", Sha256::digest(bytes.as_slice()));
                assert_eq!(actual_digest, fragment["sha256"].as_str().unwrap());
                assert_eq!(actual_digest, pin["resource_digest"].as_str().unwrap());
                assert_eq!(
                    bytes.len(),
                    pin["resource_bytes"].as_u64().unwrap() as usize
                );
                let resource: Value = serde_json::from_slice(bytes).unwrap();
                assert_eq!(resource["unit_id"], pin["unit_id"]);
                verified_split = true;
            }
        }
        if page["complete"] == true {
            assert!(page["next_cursor"].is_null());
            complete = true;
            break;
        }
        let next = page["next_cursor"]
            .as_str()
            .expect("next page cursor")
            .to_owned();
        if page_index == 0 {
            first_cursor = Some(next.clone());
        }
        cursor = Some(next);
    }
    assert!(complete, "page cursor must terminate");
    assert!(
        verified_split,
        "at least one split resource must verify by SHA-256"
    );
    let first_cursor = first_cursor.expect("two resources must require a second page");
    seen.sort_by_key(Value::to_string);
    seen.dedup();
    let mut expected = units.to_vec();
    expected.sort_by_key(Value::to_string);
    assert_eq!(seen, expected);
    let mut tampered = page_params(context, Some(&first_cursor));
    tampered["cursor"] = json!(format!("{first_cursor}x"));
    assert_eq!(
        route_error(
            &mut client,
            "query",
            "slice.pipeline.knowledge_page",
            tampered
        )
        .await["error"]["code"],
        "invalid_arguments"
    );
    let mut wrong_digest = page_params(context, None);
    wrong_digest["digest"] = json!("0".repeat(64));
    let denied = route_error(
        &mut client,
        "query",
        "slice.pipeline.knowledge_page",
        wrong_digest,
    )
    .await;
    assert!(denied["error"].is_object());
    assert!(!denied.to_string().contains("PAGED-K1-SECRET-"));
    let outsider = admin::enroll_host(&pool, None, vec![root.to_string_lossy().into_owned()])
        .await
        .unwrap();
    let outsider_config = root.join("outsider-host.json");
    host_file(&outsider_config, &outsider.auth);
    let mut outsider_client =
        Mcp::start(&socket, &outsider_config, &Uuid::new_v4().to_string(), &key).await;
    route(&mut outsider_client, "command", "workspace.open", json!({})).await;
    let denied = route_error(
        &mut outsider_client,
        "query",
        "slice.pipeline.knowledge_page",
        page_params(context, Some(&first_cursor)),
    )
    .await;
    assert!(denied["error"].is_object(), "{denied}");
    assert!(!denied.to_string().contains("PAGED-K1-SECRET-"));
    outsider_client.finish().await;

    let request = json!({"request_id":Uuid::new_v4(),"run_id":context["run"]["id"],
        "run_revision":context["run"]["revision"],"phase_id":"K1",
        "outcome":"completed","transition":"continue",
        "output":{"producer_context_id":"paged-v07-k1-public-mcp",
            "fields":{"fit":"bounded_understood","request":"prove pinned manifest",
                "parent":"current_confirmed","preflight":"current_clear",
                "authority":"authorized","acceptance_checks":"K1 advances with backend proof",
                "route":"none"},"verdict":"pass","dispositions":["satisfied"]}});
    let mut caller_owned = request.clone();
    caller_owned["request_id"] = json!(Uuid::new_v4());
    caller_owned["consumed_knowledge"] = json!({"manifest_id":manifest["id"],
        "digest":"0".repeat(64)});
    let refused = route_error(
        &mut client,
        "command",
        "slice.pipeline.phase.complete",
        caller_owned,
    )
    .await;
    assert_eq!(
        refused["error"]["refusal"]["code"], "BACKEND_DERIVED_PROOF_REQUIRED",
        "{refused}"
    );
    let completed = route(
        &mut client,
        "command",
        "slice.pipeline.phase.complete",
        request,
    )
    .await;
    assert_eq!(completed["context"]["run"]["current_phase_id"], "K2");
    let run = Uuid::parse_str(context["run"]["id"].as_str().unwrap()).unwrap();
    let (bound_id, bound_digest, evidence): (Option<Uuid>, Option<String>, Value) = sqlx::query_as(
        "SELECT knowledge_manifest_id,knowledge_manifest_digest,evidence_refs \
         FROM slice_pipeline_phase_attempts WHERE run_id=$1 AND phase_id='K1'",
    )
    .bind(run)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(bound_id.unwrap().to_string(), manifest["id"]);
    assert_eq!(bound_digest.unwrap(), manifest["digest"].as_str().unwrap());
    assert!(
        evidence
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["kind"] == "knowledge_manifest"
                && item["reference"] == manifest["id"]
                && item["digest"] == manifest["digest"])
    );
    client.finish().await;
}
