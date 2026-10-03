#[allow(dead_code)]
mod recovery_support;
#[path = "native_planning/support.rs"]
#[allow(dead_code)]
mod support;

use recovery_support::{Daemon, Mcp, host_file, private_temp, tagged_url};
use serde_json::json;
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use support::{route, route_error};
use tect_postgres::admin;
use uuid::Uuid;

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn artifacts_page_unicode_and_preserve_workspace_collaboration_and_isolation() {
    let pool = PgPool::connect(&std::env::var("TECT_TEST_ADMIN_URL").expect("isolated admin URL"))
        .await
        .unwrap();
    let runtime = std::env::var("TECT_TEST_RUNTIME_URL").expect("isolated runtime URL");
    let role = std::env::var("TECT_TEST_RUNTIME_ROLE").expect("isolated role");
    admin::migrate(&pool, &role).await.unwrap();
    let temp = private_temp();
    let root = temp.path().canonicalize().unwrap();
    let socket = root.join("artifacts.sock");
    let _daemon = Daemon::start(&tagged_url(&runtime, "artifact-pages"), socket.clone()).await;
    let owner = admin::enroll_host(&pool, None, vec![root.to_string_lossy().into_owned()])
        .await
        .unwrap();
    let config = root.join("owner.json");
    host_file(&config, &owner.auth);
    let key = format!("artifacts-{}", Uuid::new_v4());
    let mut client = Mcp::start(&socket, &config, &Uuid::new_v4().to_string(), &key).await;
    client.call("open_workspace", json!({})).await;
    let body = "é🙂aé🙂z";
    let register = json!({"request_id":Uuid::new_v4(),"digest":format!("{:x}",Sha256::digest(body.as_bytes())),
        "size":body.len(),"format":"text/plain","provenance":"Owned synthetic integration fixture","target":"local Unicode artifact"});
    let registered = route(
        &mut client,
        "command",
        "slice.pipeline.evidence_artifact.register",
        register.clone(),
    )
    .await;
    let artifact = registered["artifact"].clone();
    assert_eq!(artifact["readiness"], "uploading");
    let replay = route(
        &mut client,
        "command",
        "slice.pipeline.evidence_artifact.register",
        register.clone(),
    )
    .await;
    assert_eq!(replay["artifact"], artifact);
    assert_eq!(replay["replay"], true);
    let mut conflict = register;
    conflict["target"] = json!("changed");
    assert_eq!(
        route_error(
            &mut client,
            "command",
            "slice.pipeline.evidence_artifact.register",
            conflict
        )
        .await["error"]["code"],
        "input_conflict"
    );
    let finalize = json!({"request_id":Uuid::new_v4(),"artifact_id":artifact["artifact_id"],"revision":artifact["revision"],"body":body});
    let finalized = route(
        &mut client,
        "command",
        "slice.pipeline.evidence_artifact.finalize",
        finalize.clone(),
    )
    .await;
    assert_eq!(finalized["artifact"]["readiness"], "ready");
    assert_eq!(
        route(
            &mut client,
            "command",
            "slice.pipeline.evidence_artifact.finalize",
            finalize
        )
        .await["replay"],
        true
    );
    let read = json!({"artifact_id":artifact["artifact_id"],"revision":artifact["revision"],"offset":0,"limit":4});
    let mut offset = 0;
    let mut reconstructed = String::new();
    loop {
        let mut params = read.clone();
        params["offset"] = json!(offset);
        let page = route(
            &mut client,
            "query",
            "slice.pipeline.evidence_artifact.read",
            params,
        )
        .await;
        let fragment = page["fragment"].as_str().unwrap();
        assert!(fragment.len() <= 4);
        reconstructed.push_str(fragment);
        if page["complete"] == true {
            assert!(page["next_offset"].is_null());
            break;
        }
        let next = page["next_offset"].as_u64().unwrap();
        assert!(next > offset);
        offset = next;
    }
    assert_eq!(reconstructed, body);
    for (offset, limit) in [(1, 4), (0, 1), (2, 3), (body.len() + 1, 4)] {
        let mut params = read.clone();
        params["offset"] = json!(offset);
        params["limit"] = json!(limit);
        let error = route_error(
            &mut client,
            "query",
            "slice.pipeline.evidence_artifact.read",
            params,
        )
        .await;
        assert_eq!(error["error"]["code"], "invalid_arguments");
        assert_eq!(error["error"]["refusal"]["code"], "INPUT_SCHEMA_INVALID");
    }
    let mut end = read.clone();
    end["offset"] = json!(body.len());
    let end_page = route(
        &mut client,
        "query",
        "slice.pipeline.evidence_artifact.read",
        end,
    )
    .await;
    assert_eq!(end_page["fragment"], "");
    assert_eq!(end_page["complete"], true);
    let mut peer = Mcp::start(&socket, &config, &Uuid::new_v4().to_string(), &key).await;
    peer.call("open_workspace", json!({})).await;
    assert_eq!(
        route(
            &mut peer,
            "query",
            "slice.pipeline.evidence_artifact.read",
            read.clone()
        )
        .await["fragment"],
        "é"
    );
    let mut workspace_peer = Mcp::start(
        &socket,
        &config,
        &Uuid::new_v4().to_string(),
        &format!("other-{}", Uuid::new_v4()),
    )
    .await;
    workspace_peer.call("open_workspace", json!({})).await;
    assert_eq!(
        route_error(
            &mut workspace_peer,
            "query",
            "slice.pipeline.evidence_artifact.read",
            read.clone()
        )
        .await["error"]["code"],
        "not_found"
    );
    let foreign = admin::enroll_host(&pool, None, vec![root.to_string_lossy().into_owned()])
        .await
        .unwrap();
    assert_ne!(foreign.tenant_id, owner.tenant_id);
    assert_ne!(foreign.principal_id, owner.principal_id);
    let foreign_config = root.join("foreign.json");
    host_file(&foreign_config, &foreign.auth);
    let mut foreign_peer =
        Mcp::start(&socket, &foreign_config, &Uuid::new_v4().to_string(), &key).await;
    foreign_peer.call("open_workspace", json!({})).await;
    assert_eq!(
        route_error(
            &mut foreign_peer,
            "query",
            "slice.pipeline.evidence_artifact.read",
            read
        )
        .await["error"]["code"],
        "not_found"
    );
    eprintln!(
        "artifact {}: Unicode reconstruction, safe refusals, separate-session same-principal sharing, workspace/tenant isolation passed",
        artifact["artifact_id"]
    );
}
