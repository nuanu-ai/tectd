//! Opt-in continuation of an already persisted live Scope advisory.
//! This binary contains no provider adapter and never calls scope.advisory.request.
#[allow(dead_code)]
mod recovery_support;
#[path = "native_planning/support.rs"]
#[allow(dead_code)]
mod support;

use recovery_support::{Daemon, Mcp, private_temp, tagged_url};
use serde_json::json;
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use std::{
    fs,
    io::Write,
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::Path,
};
use support::{id, route};
use tect_domain::{HostAuth, ScopeCandidateDraft};
use uuid::Uuid;

fn required(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("{name} is required"))
}

fn private_auth(name: &str) -> (String, HostAuth) {
    let path = required(name);
    let file = Path::new(&path);
    assert!(
        file.is_absolute() && file.is_file(),
        "{name} must name an existing absolute file"
    );
    assert_eq!(
        fs::metadata(file).unwrap().permissions().mode() & 0o077,
        0,
        "{name} must be private"
    );
    let auth: HostAuth = serde_json::from_slice(&fs::read(file).unwrap())
        .unwrap_or_else(|_| panic!("{name} is not a HostAuth file"));
    (path, auth)
}

fn check_private_output(path: &Path) {
    assert!(
        path.is_absolute() && !path.exists(),
        "private output must be a new absolute file"
    );
    let parent = path.parent().unwrap();
    assert!(parent.is_dir());
    assert_eq!(
        fs::metadata(parent).unwrap().permissions().mode() & 0o077,
        0,
        "private output parent must be owner-only"
    );
}

fn write_private(path: &Path, bytes: &[u8]) {
    check_private_output(path);
    let parent = path.parent().unwrap();
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .unwrap();
    file.write_all(bytes).unwrap();
    file.sync_all().unwrap();
    fs::File::open(parent).unwrap().sync_all().unwrap();
}

async fn assert_host(pool: &PgPool, auth: &HostAuth, tenant: Uuid, role: &str) -> Uuid {
    let row: (Uuid, Uuid, String, bool, String) = sqlx::query_as(
        "SELECT h.tenant_id,h.principal_id,h.credential_digest,h.revoked,p.role \
         FROM hosts h JOIN principals p ON (p.tenant_id,p.id)=(h.tenant_id,h.principal_id) \
         WHERE h.id=$1",
    )
    .bind(auth.host_id)
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(row.0, tenant, "host tenant differs");
    assert_eq!(row.4, role, "host role differs");
    assert!(!row.3, "host is revoked");
    assert_eq!(
        row.2,
        format!("{:x}", Sha256::digest(auth.credential.as_bytes())),
        "host credential differs"
    );
    row.1
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "explicit JEV_FOLLOWTHROUGH_MODE=run and exact retained opportunity/auth/draft; mutates only the retained test DB"]
async fn persisted_live_advice_through_public_mcp() {
    assert_eq!(required("JEV_FOLLOWTHROUGH_MODE"), "run");
    let opportunity = Uuid::parse_str(&required("JEV_FOLLOWTHROUGH_OPPORTUNITY_ID")).unwrap();
    let tenant = Uuid::parse_str(&required("JEV_FOLLOWTHROUGH_TENANT_ID")).unwrap();
    let workspace = Uuid::parse_str(&required("JEV_FOLLOWTHROUGH_WORKSPACE_ID")).unwrap();
    let expected_provider = required("JEV_FOLLOWTHROUGH_PROVIDER");
    let expected_model = required("JEV_FOLLOWTHROUGH_MODEL");
    let alternative_key = required("JEV_FOLLOWTHROUGH_ALTERNATIVE_KEY");
    let selected_id = required("JEV_FOLLOWTHROUGH_SELECTED_ID");
    let rationale = required("JEV_FOLLOWTHROUGH_SELECTION_RATIONALE");
    assert!(!rationale.trim().is_empty());
    let (owner_path, owner_auth) = private_auth("JEV_FOLLOWTHROUGH_OWNER_AUTH_FILE");
    let (verifier_path, verifier_auth) = private_auth("JEV_FOLLOWTHROUGH_VERIFIER_AUTH_FILE");
    assert_ne!(owner_auth.host_id, verifier_auth.host_id);
    let draft_path = required("JEV_FOLLOWTHROUGH_DRAFT_FILE");
    assert!(Path::new(&draft_path).is_absolute());
    let draft: ScopeCandidateDraft = serde_json::from_slice(&fs::read(&draft_path).unwrap())
        .expect("exact source-authored draft JSON required");
    draft.validate().unwrap();
    let draft = serde_json::to_value(draft).unwrap();

    let admin_url = required("JEV_FOLLOWTHROUGH_ADMIN_URL");
    let runtime_url = required("JEV_FOLLOWTHROUGH_RUNTIME_URL");
    let pool = PgPool::connect(&admin_url).await.unwrap();
    let version: String = sqlx::query_scalar("SHOW server_version_num")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(version.parse::<i32>().unwrap() / 10_000, 18);
    let owner_actor = assert_host(&pool, &owner_auth, tenant, "owner").await;
    let verifier_actor = assert_host(&pool, &verifier_auth, tenant, "verifier").await;
    assert_ne!(owner_actor, verifier_actor);
    let verifier_granted: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM memberships WHERE tenant_id=$1 AND workspace_id=$2 AND principal_id=$3)",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(verifier_actor)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(verifier_granted, "verifier lacks exact workspace grant");
    let (candidate, owner_session, source_revision, state, reason, capability, decision):
        (Uuid, Uuid, String, String, String, String, String) = sqlx::query_as(
        "SELECT work_item_id,session_id,source_revision,state,primary_reason,capability,decision_point \
         FROM advisory_opportunity WHERE id=$1 AND tenant_id=$2 AND workspace_id=$3 \
         AND work_item_kind='scope_candidate_set'",
    ).bind(opportunity).bind(tenant).bind(workspace).fetch_one(&pool).await.unwrap();
    assert_eq!(
        (state.as_str(), reason.as_str()),
        ("advised", "provider_response")
    );
    assert_eq!(
        (capability.as_str(), decision.as_str()),
        (
            "scope_decomposition",
            "scope.decomposition.before_selection"
        )
    );
    let (owner_native, owner_host, workspace_key, session_actor): (String, Uuid, String, Uuid) =
        sqlx::query_as(
            "SELECT s.native_session_id,s.host_id,w.key,h.principal_id FROM agent_sessions s \
             JOIN workspaces w ON (w.tenant_id,w.id)=(s.tenant_id,s.workspace_id) \
             JOIN hosts h ON (h.tenant_id,h.id)=(s.tenant_id,s.host_id) \
             WHERE s.tenant_id=$1 AND s.workspace_id=$2 AND s.id=$3 AND NOT s.revoked",
        )
        .bind(tenant)
        .bind(workspace)
        .bind(owner_session)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(
        owner_host, owner_auth.host_id,
        "original owner host required"
    );
    assert_eq!(session_actor, owner_actor);
    let dispatches: Vec<(i32, String, String, String, String, Option<String>)> = sqlx::query_as(
        "SELECT attempt_number,provider,model,state,send_certainty,outcome FROM advisory_dispatch \
         WHERE tenant_id=$1 AND workspace_id=$2 AND opportunity_id=$3",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(opportunity)
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(dispatches.len(), 1, "exactly one prior dispatch required");
    assert_eq!(
        dispatches[0],
        (
            1,
            expected_provider,
            expected_model,
            "sealed".into(),
            "sent".into(),
            Some("provider_response".into())
        )
    );
    let advice_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM advisory_scope_advice WHERE tenant_id=$1 AND workspace_id=$2 \
         AND opportunity_id=$3 AND candidate_set_id=$4",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(opportunity)
    .bind(candidate)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(advice_count, 1, "persisted guarded advice required");
    let prior_effects: (i64, i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM advisory_scope_disposition WHERE opportunity_id=$1), \
                (SELECT count(*) FROM advisory_scope_caller_link WHERE opportunity_id=$1), \
                (SELECT count(*) FROM advisory_scope_verifier_receipt WHERE opportunity_id=$1)",
    )
    .bind(opportunity)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(prior_effects, (0, 0, 0), "opportunity already continued");

    let temp = private_temp();
    let socket = temp.path().join("jev-followthrough.sock");
    let runtime = tagged_url(
        &runtime_url,
        &format!("jev-followthrough-{}", Uuid::new_v4()),
    );
    let mut daemon = Daemon::start(&runtime, socket.clone()).await;
    let mut owner = Mcp::start(
        &socket,
        Path::new(&owner_path),
        &owner_native,
        &workspace_key,
    )
    .await;
    let opened = route(&mut owner, "command", "workspace.open", json!({})).await;
    assert_eq!(id(&opened["workspace"]["id"]), workspace);
    let detail = route(
        &mut owner,
        "query",
        "candidate.advisory.get",
        json!({"candidate_set_id":candidate,"opportunity_id":opportunity}),
    )
    .await;
    let projection = &detail["scope_decomposition"];
    assert_eq!(projection["version"], 1);
    let manifest = &projection["manifest"];
    let advice = &projection["advice"];
    assert_eq!(id(&manifest["source"]["candidate_set_id"]), candidate);
    assert_eq!(
        manifest["source"]["candidate_set_revision"].to_string(),
        source_revision
    );
    assert_eq!(advice["opportunity_id"], opportunity.to_string());
    assert_eq!(advice["ranked_ids"].as_array().unwrap().len(), 2);
    // JEV preferred both alternatives in this evidence. The caller explicitly
    // supersedes that non-exclusive advice with the frozen deterministic baseline.
    assert!(
        manifest["emitted"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["id"] == selected_id)
    );
    assert_eq!(
        selected_id, manifest["baseline_id"],
        "deterministic supersession must select the frozen baseline"
    );
    let context = route(
        &mut owner,
        "query",
        "scope.candidates.context",
        json!({"candidate_set_id":candidate,"view":"candidates","limit":25}),
    )
    .await;
    let context = &context["context"];
    assert_eq!(
        context["candidate_set"]["revision"],
        manifest["source"]["candidate_set_revision"]
    );
    assert_eq!(context["snapshot"]["id"], manifest["source"]["snapshot_id"]);
    let revision = context["candidate_set"]["revision"].as_i64().unwrap();
    let snapshot = context["snapshot"]["id"].clone();
    let input_cursor = context["candidate_set"]["input_cursor"].as_i64().unwrap();
    assert_eq!(
        input_cursor,
        manifest["source"]["input_cursor"].as_i64().unwrap()
    );
    // The DB-bound selected-save route resolves the submitted draft and compares
    // it byte-for-byte as a typed material value with this frozen alternative.
    let items = manifest["emitted"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| {
            json!({
                "alternative_id":item["id"],
                "state":if item["id"] == selected_id {"selected"} else {"not_selected"}
            })
        })
        .collect::<Vec<_>>();
    let disposition = route(
        &mut owner,
        "command",
        "scope.advisory.disposition",
        json!({
            "opportunity_id":opportunity,"candidate_set_id":candidate,"request_id":Uuid::new_v4(),
            "advice_id":advice["id"],"expected_revision":0,
            "action":"supersede_with_deterministic_choice",
            "selected_id":selected_id,"items":items,
            "rationale":rationale
        }),
    )
    .await;
    let save_request = Uuid::new_v4();
    let saved = route(
        &mut owner,
        "command",
        "scope.candidates.save",
        json!({
            "kind":"draft","candidate_set_id":candidate,"revision":revision,
            "snapshot_id":snapshot,"input_cursor":input_cursor,"request_id":save_request,
            "selected_advisory":{"opportunity_id":opportunity,"disposition_id":disposition["id"],
                "selected_id":selected_id,"alternative_key":alternative_key},
            "draft":draft
        }),
    )
    .await;
    assert_eq!(saved["context"]["candidate_set"]["revision"], revision + 1);
    let (link_id, target_revision, receipt_id, status): (Uuid, i64, Uuid, String) = sqlx::query_as(
        "SELECT l.link_id,l.caller_result_revision,p.receipt_id,p.status FROM advisory_scope_caller_link l \
         JOIN advisory_scope_preservation_receipt p ON (p.tenant_id,p.workspace_id,p.receipt_id)= \
             (l.tenant_id,l.workspace_id,l.preservation_receipt_id) \
         WHERE l.tenant_id=$1 AND l.workspace_id=$2 AND l.opportunity_id=$3 \
         AND l.candidate_set_id=$4 AND l.request_id=$5 AND l.actor_id=$6 AND l.session_id=$7",
    ).bind(tenant).bind(workspace).bind(opportunity).bind(candidate).bind(save_request)
        .bind(owner_actor).bind(owner_session).fetch_one(&pool).await.unwrap();
    assert_eq!(status, "passed");
    assert!(!receipt_id.is_nil());

    let mut verifier = Mcp::start(
        &socket,
        Path::new(&verifier_path),
        &Uuid::new_v4().to_string(),
        &workspace_key,
    )
    .await;
    let verifier_opened = route(&mut verifier, "command", "workspace.open", json!({})).await;
    assert_eq!(id(&verifier_opened["workspace"]["id"]), workspace);
    let verifier_read = route(
        &mut verifier,
        "query",
        "candidate.advisory.get",
        json!({"candidate_set_id":candidate,"opportunity_id":opportunity}),
    )
    .await;
    assert!(verifier_read["opportunity"].is_object());
    assert!(verifier_read.get("scope_decomposition").is_none());
    let verified = route(
        &mut verifier,
        "command",
        "candidate.advisory.verify",
        json!({
            "request_id":Uuid::new_v4(),"opportunity_id":opportunity,"candidate_set_id":candidate,
            "caller_link_id":link_id,"caller_receipt_request_id":save_request,
            "target_revision":target_revision
        }),
    )
    .await;
    assert_eq!(verified["observation"]["status"], "passed");
    assert_eq!(
        verified["observation"]["qualification"],
        "independently_observed"
    );
    assert_eq!(
        verified["observation"]["actor_id"],
        verifier_actor.to_string()
    );
    assert_eq!(verified["establishes_independent_approval"], false);
    assert_eq!(verified["establishes_current_acceptance"], false);
    let audit = route(
        &mut verifier,
        "query",
        "candidate.advisory.audit",
        json!({"candidate_set_id":candidate,"limit":50}),
    )
    .await;
    assert!(audit.to_string().contains("independently_observed"));
    let after_dispatches: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM advisory_dispatch WHERE tenant_id=$1 AND workspace_id=$2 AND opportunity_id=$3",
    ).bind(tenant).bind(workspace).bind(opportunity).fetch_one(&pool).await.unwrap();
    assert_eq!(
        after_dispatches, 1,
        "downstream MCP must not dispatch again"
    );
    println!(
        "followthrough opportunity={opportunity} candidate_set={candidate} \
        disposition={} caller_link={link_id} preservation={receipt_id} verification=passed dispatches=1",
        disposition["id"]
    );
    verifier.finish().await;
    owner.finish().await;
    daemon.crash().await;
    daemon.remove_owned_stale_socket();
}

/// Separate, explicit recovery of the original fixture host after the one-shot
/// test's TempDir has removed its credential file. No session, advice, dispatch,
/// or candidate row is changed. This is an operator action, not a test setup.
#[tokio::test]
#[ignore = "explicit JEV_FOLLOWTHROUGH_MODE=recover and exact isolated DB fingerprint; rotates one original fixture host credential"]
async fn recover_original_owner_host_credential() {
    assert_eq!(required("JEV_FOLLOWTHROUGH_MODE"), "recover");
    let opportunity = Uuid::parse_str(&required("JEV_FOLLOWTHROUGH_OPPORTUNITY_ID")).unwrap();
    let tenant = Uuid::parse_str(&required("JEV_FOLLOWTHROUGH_TENANT_ID")).unwrap();
    let workspace = Uuid::parse_str(&required("JEV_FOLLOWTHROUGH_WORKSPACE_ID")).unwrap();
    let expected_session =
        Uuid::parse_str(&required("JEV_FOLLOWTHROUGH_OWNER_SESSION_ID")).unwrap();
    let expected_host = Uuid::parse_str(&required("JEV_FOLLOWTHROUGH_OWNER_HOST_ID")).unwrap();
    let expected_old_digest = required("JEV_FOLLOWTHROUGH_OLD_CREDENTIAL_DIGEST");
    assert_eq!(expected_old_digest.len(), 64);
    assert!(
        expected_old_digest
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    );
    let expected_system = required("JEV_FOLLOWTHROUGH_SYSTEM_IDENTIFIER");
    let expected_database_oid = required("JEV_FOLLOWTHROUGH_DATABASE_OID")
        .parse::<i64>()
        .unwrap();
    let auth_path = required("JEV_FOLLOWTHROUGH_OWNER_AUTH_FILE");
    let receipt_path = required("JEV_FOLLOWTHROUGH_RECOVERY_RECEIPT_FILE");
    assert_ne!(auth_path, receipt_path);
    assert!(
        Path::new(&auth_path).is_absolute() && !Path::new(&auth_path).exists(),
        "recover only when original auth file is absent"
    );
    assert!(Path::new(&receipt_path).is_absolute() && !Path::new(&receipt_path).exists());
    check_private_output(Path::new(&auth_path));
    check_private_output(Path::new(&receipt_path));
    let admin_url = required("JEV_FOLLOWTHROUGH_ADMIN_URL");
    let pool = PgPool::connect(&admin_url).await.unwrap();
    let version: String = sqlx::query_scalar("SHOW server_version_num")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(version.parse::<i32>().unwrap() / 10_000, 18);
    let (system, database_oid): (String, i64) = sqlx::query_as(
        "SELECT system_identifier::text,(SELECT oid::bigint FROM pg_catalog.pg_database \
         WHERE datname=pg_catalog.current_database()) FROM pg_catalog.pg_control_system()",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        (system.as_str(), database_oid),
        (expected_system.as_str(), expected_database_oid),
        "isolated database identity differs"
    );
    let mut tx = pool.begin().await.unwrap();
    let (actual_session, actual_host, actor, state, reason): (Uuid, Uuid, Uuid, String, String) =
        sqlx::query_as(
            "SELECT o.session_id,s.host_id,o.authorized_actor_id,o.state,o.primary_reason \
             FROM advisory_opportunity o JOIN agent_sessions s \
               ON (s.tenant_id,s.workspace_id,s.id)=(o.tenant_id,o.workspace_id,o.session_id) \
             WHERE o.id=$1 AND o.tenant_id=$2 AND o.workspace_id=$3 \
               AND o.work_item_kind='scope_candidate_set' AND NOT s.revoked",
        )
        .bind(opportunity)
        .bind(tenant)
        .bind(workspace)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    assert_eq!(
        (actual_session, actual_host),
        (expected_session, expected_host)
    );
    assert_eq!(
        (state.as_str(), reason.as_str()),
        ("advised", "provider_response")
    );
    let (host_actor, old_digest, revoked, role): (Uuid, String, bool, String) = sqlx::query_as(
        "SELECT h.principal_id,h.credential_digest,h.revoked,p.role FROM hosts h \
         JOIN principals p ON (p.tenant_id,p.id)=(h.tenant_id,h.principal_id) \
         WHERE h.tenant_id=$1 AND h.id=$2 FOR UPDATE OF h",
    )
    .bind(tenant)
    .bind(expected_host)
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    assert_eq!(host_actor, actor);
    assert_eq!(role, "owner");
    assert!(!revoked);
    assert_eq!(old_digest, expected_old_digest);
    let (dispatch_count, advice_count, disposition_count): (i64, i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM advisory_dispatch WHERE tenant_id=$1 AND workspace_id=$2 AND opportunity_id=$3), \
                (SELECT count(*) FROM advisory_scope_advice WHERE tenant_id=$1 AND workspace_id=$2 AND opportunity_id=$3), \
                (SELECT count(*) FROM advisory_scope_disposition WHERE tenant_id=$1 AND workspace_id=$2 AND opportunity_id=$3)",
    ).bind(tenant).bind(workspace).bind(opportunity).fetch_one(&mut *tx).await.unwrap();
    assert_eq!((dispatch_count, advice_count, disposition_count), (1, 1, 0));
    let credential = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
    let auth = HostAuth {
        host_id: expected_host,
        credential,
    };
    let new_digest = format!("{:x}", Sha256::digest(auth.credential.as_bytes()));
    assert_ne!(new_digest, old_digest);
    write_private(Path::new(&auth_path), &serde_json::to_vec(&auth).unwrap());
    let updated = sqlx::query(
        "UPDATE hosts SET credential_digest=$1 WHERE id=$2 AND tenant_id=$3 \
         AND principal_id=$4 AND credential_digest=$5 AND NOT revoked",
    )
    .bind(&new_digest)
    .bind(expected_host)
    .bind(tenant)
    .bind(actor)
    .bind(&old_digest)
    .execute(&mut *tx)
    .await
    .unwrap();
    assert_eq!(updated.rows_affected(), 1);
    tx.commit().await.unwrap();
    let stored: String =
        sqlx::query_scalar("SELECT credential_digest FROM hosts WHERE tenant_id=$1 AND id=$2")
            .bind(tenant)
            .bind(expected_host)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(stored, new_digest);
    let receipt = json!({
        "action":"isolated_fixture_owner_credential_rotation",
        "opportunity_id":opportunity,"tenant_id":tenant,"workspace_id":workspace,
        "session_id":expected_session,"host_id":expected_host,"actor_id":actor,
        "system_identifier":system,"database_oid":database_oid,
        "old_credential_digest":old_digest,"new_credential_digest":new_digest,
        "status":"committed"
    });
    write_private(
        Path::new(&receipt_path),
        &serde_json::to_vec_pretty(&receipt).unwrap(),
    );
    println!(
        "isolated owner credential recovered: opportunity={opportunity} host={expected_host} \
        session={expected_session} dispatches=1 receipt={receipt_path}"
    );
}
