//! Owned host wire backed by real PostgreSQL source; all business approvals,
//! ranking, capability/economic facts and planning scaffold are TEST CONTROLS.
use crate::{
    PgStore,
    technical_decision_comparison_pg_tests::{ControlFixture, NoExternal, control_fixture},
};
use async_trait::async_trait;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use std::{
    fs::{self, OpenOptions},
    io::Write,
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tect_application::*;
use tect_domain::*;
use tokio::net::UnixListener;
use uuid::Uuid;
mod fixture;
const PROMPT: &str = "Return exactly JEV_CONTROL_OK. This is an isolated source-to-host routing control test; do not use tools or change files.";
fn sha(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn selection_params(r: &PrepareModelRouteHostSelection) -> Value {
    json!({"preparation_request_key":r.preparation_request_key,"decision_id":r.decision_id,"disposition_id":r.disposition_id,"expected_task_id":r.expected_task_id,"expected_task_revision":r.expected_task_revision,"expected_work_context_digest":r.expected_work_context_digest,"expected_catalogue_digest":r.expected_catalogue_digest,"selected_route_id":r.selected_route_id,"input_sha256":r.input_sha256,"invocation_key":r.invocation_key})
}
async fn query(
    socket: &Path,
    context: &RequestContext,
    route: &str,
    params: Value,
) -> Result<Value> {
    // The owned Unix protocol carries internal APIv2 tools. Named query
    // decoding belongs to the MCP bridge and is separately schema-tested.
    let tool = match route {
        "matrix.technical.compare" => "compare_technical_delivery_mechanisms",
        "model.route.host.selection" => "prepare_model_route_host_selection",
        _ => return Err(Error::InvalidArguments),
    };
    tect_host::call_tool(socket, context, tool, params).await
}
fn private_file(path: &Path, bytes: &[u8]) {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .unwrap();
    file.write_all(bytes).unwrap();
    file.sync_all().unwrap();
    assert_eq!(
        fs::metadata(path).unwrap().permissions().mode() & 0o777,
        0o600
    );
}
async fn effects(pool: &PgPool, tenant: Uuid) -> (i64, i64, i64, i64, i64) {
    sqlx::query_as("SELECT (SELECT count(*) FROM advisory_dispatch WHERE tenant_id=$1),(SELECT count(*) FROM advisory_opportunity WHERE tenant_id=$1),(SELECT count(*) FROM matrix_task_revisions WHERE tenant_id=$1),(SELECT count(*) FROM model_route_advisory_attempts WHERE tenant_id=$1),(SELECT count(*) FROM model_route_dispositions WHERE tenant_id=$1)").bind(tenant).fetch_one(pool).await.unwrap()
}

#[tokio::test]
#[ignore = "requires exact owned PG18.6; optional explicit host-wire harness pin"]
async fn authenticated_host_wire_pg_s02_s05_control_fixture() {
    let f = fixture::build().await;
    let c = &f.control;
    let directory = PathBuf::from("/private/tmp").join(format!("jev-wire-{}", Uuid::new_v4()));
    fs::create_dir(&directory).unwrap();
    fs::set_permissions(&directory, fs::Permissions::from_mode(0o700)).unwrap();
    let socket = directory.join("host.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    fs::set_permissions(&socket, fs::Permissions::from_mode(0o600)).unwrap();
    let server = tokio::spawn(tect_host::serve(listener, f.service.clone()));
    let before = effects(&c.admin_pool, c.owner.tenant_id).await;
    let s02=query(&socket,&c.compare_context,"matrix.technical.compare",json!({"task_id":c.task,"expected_task_revision":1,"operating_verification_digest":c.request.operating_verification_digest,"evidence_reference":{"artifact_id":c.request.evidence_reference.artifact_id,"artifact_version":1,"content_sha256":c.request.evidence_reference.content_sha256}})).await.unwrap();
    assert_eq!(s02["state"], "compared");
    assert_eq!(s02["comparison"]["eligible_approach_ids"], json!(["reuse"]));
    let current = query(
        &socket,
        &c.compare_context,
        "model.route.host.selection",
        selection_params(&f.selection_request),
    )
    .await
    .unwrap();
    assert_eq!(
        current["authorization_scope"],
        "current_authenticated_read_only_snapshot"
    );
    let material = current["material_json"].as_str().unwrap();
    assert_eq!(current["material_sha256"], sha(material.as_bytes()));
    assert_eq!(
        serde_json::from_str::<Value>(material).unwrap(),
        current["material"]
    );
    assert_eq!(current["material"]["selected_route"]["model"], "gpt-6-luna");
    assert_eq!(current["material"]["selected_route"]["effort"], "xhigh");
    assert!(current["material"]["preparation"]["work"]["context_authority"].is_object());
    assert_eq!(
        current["material"]["source_binding"]["source_recorded_by_actor_id"],
        c.owner.principal_id.to_string()
    );
    assert_eq!(
        effects(&c.admin_pool, c.owner.tenant_id).await,
        before,
        "queries must not persist effects"
    );
    let mut wrong = f.selection_request.clone();
    wrong.expected_work_context_digest = "f".repeat(64);
    assert_eq!(
        query(
            &socket,
            &c.compare_context,
            "model.route.host.selection",
            selection_params(&wrong)
        )
        .await,
        Err(Error::InputConflict)
    );
    let mut noaccepted = f.selection_request.clone();
    noaccepted.disposition_id = Uuid::new_v4();
    assert_eq!(
        query(
            &socket,
            &c.compare_context,
            "model.route.host.selection",
            selection_params(&noaccepted)
        )
        .await,
        Err(Error::NotFound)
    );
    let mut missing = c.compare_context.clone();
    missing.native_session_id = Uuid::new_v4().to_string();
    assert_eq!(
        query(
            &socket,
            &missing,
            "model.route.host.selection",
            selection_params(&f.selection_request)
        )
        .await,
        Err(Error::WorkspaceNotOpen)
    );
    sqlx::query("UPDATE agent_sessions SET revoked=true WHERE id=$1")
        .bind(c.sessions[1])
        .execute(&c.admin_pool)
        .await
        .unwrap();
    assert_eq!(
        query(
            &socket,
            &c.compare_context,
            "model.route.host.selection",
            selection_params(&f.selection_request)
        )
        .await,
        Err(Error::SessionRevoked)
    );
    sqlx::query("UPDATE agent_sessions SET revoked=false WHERE id=$1")
        .bind(c.sessions[1])
        .execute(&c.admin_pool)
        .await
        .unwrap();
    sqlx::query("UPDATE slice_candidate_sets SET revision=revision+1 WHERE id=$1")
        .bind(f.candidate_set)
        .execute(&c.admin_pool)
        .await
        .unwrap();
    assert!(matches!(
        query(
            &socket,
            &c.compare_context,
            "model.route.host.selection",
            selection_params(&f.selection_request)
        )
        .await,
        Err(Error::StaleContext | Error::StaleRevision)
    ));
    sqlx::query("UPDATE slice_candidate_sets SET revision=revision-1 WHERE id=$1")
        .bind(f.candidate_set)
        .execute(&c.admin_pool)
        .await
        .unwrap();

    // S05 takes candidate SHARE before task; a held task must abort promptly,
    // release candidate SHARE, create no new receipt, then allow fresh retry.
    let store = PgStore::from_pool(c.runtime.clone());
    let mut holder = store.begin(TransactionMode::ReadWrite).await.unwrap();
    holder.authenticate(&c.owner.auth).await.unwrap();
    holder.set_tenant(c.owner.tenant_id).await.unwrap();
    holder
        .lock_matrix_task(c.workspace, c.task)
        .await
        .unwrap()
        .unwrap();
    let losing = tokio::time::timeout(
        Duration::from_secs(2),
        query(
            &socket,
            &c.compare_context,
            "model.route.host.selection",
            selection_params(&f.selection_request),
        ),
    )
    .await
    .unwrap();
    assert!(matches!(
        losing,
        Err(Error::StaleRevision | Error::StaleContext)
    ));
    let mut candidate_winner = c.runtime.begin().await.unwrap();
    sqlx::query("SELECT set_config('tect.tenant_id',$1,true)")
        .bind(c.owner.tenant_id.to_string())
        .execute(&mut *candidate_winner)
        .await
        .unwrap();
    sqlx::query("SELECT id FROM slice_candidate_sets WHERE id=$1 FOR UPDATE NOWAIT")
        .bind(f.candidate_set)
        .fetch_one(&mut *candidate_winner)
        .await
        .unwrap();
    candidate_winner.commit().await.unwrap();
    holder.commit().await.unwrap();
    query(
        &socket,
        &c.compare_context,
        "model.route.host.selection",
        selection_params(&f.selection_request),
    )
    .await
    .unwrap();
    assert_eq!(effects(&c.admin_pool, c.owner.tenant_id).await, before);

    let auth = directory.join("host-auth.json");
    private_file(
        &auth,
        serde_json::to_string(&c.owner.auth).unwrap().as_bytes(),
    );
    let ledger = directory.join("ledger");
    fs::create_dir(&ledger).unwrap();
    fs::set_permissions(&ledger, fs::Permissions::from_mode(0o700)).unwrap();
    let prompt = directory.join("prompt.txt");
    private_file(&prompt, PROMPT.as_bytes());
    let mut pins = selection_params(&f.selection_request);
    pins.as_object_mut().unwrap().remove("input_sha256");
    pins["expected_workspace_id"] = json!(c.workspace);
    pins["expected_actor_id"] = json!(c.owner.principal_id);
    pins["expected_session_id"] = json!(c.sessions[1]);
    let pinpath = directory.join("pins.json");
    private_file(&pinpath, serde_json::to_string(&pins).unwrap().as_bytes());
    assert_ne!(
        c.compare_context.native_session_id,
        c.sessions[1].to_string()
    );
    let repository = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    let python=tokio::process::Command::new("python3").current_dir(repository).env("WIRE_CONTROL_DIR",&directory).env("WIRE_WORKSPACE_KEY",&c.compare_context.workspace_key).env("WIRE_NATIVE_SESSION",&c.compare_context.native_session_id)
        .arg("-c").arg("import hashlib,json,os; from pathlib import Path; from scripts.authenticated_caller_source import AuthenticatedCurrentSource,_host_adapter_context; from scripts.caller_host_routing import CallerRoutingRequest; d=Path(os.environ['WIRE_CONTROL_DIR']); p=json.loads((d/'pins.json').read_text()); ctx=_host_adapter_context(socket_path=d/'host.sock',config_path=d/'host-auth.json',workspace_key=os.environ['WIRE_WORKSPACE_KEY'],native_session_id=os.environ['WIRE_NATIVE_SESSION'],workspace_id=p['expected_workspace_id'],actor_id=p['expected_actor_id'],session_id=p['expected_session_id']); r=CallerRoutingRequest(**{k:p[k] for k in CallerRoutingRequest.__dataclass_fields__}); s=AuthenticatedCurrentSource(ctx).resolve_current(r,input_sha256=hashlib.sha256((d/'prompt.txt').read_bytes()).hexdigest()); m=json.loads(s.material_json); assert m['selected_route']['model']=='gpt-6-luna' and m['selected_route']['effort']=='xhigh'; print('PYTHON_REAL_SOURCE_DECODER_OK '+s.material_sha256)")
        .output().await.unwrap();
    assert!(
        python.status.success(),
        "Python source adapter failed: {}",
        String::from_utf8_lossy(&python.stderr)
    );
    eprintln!("{}", String::from_utf8_lossy(&python.stdout).trim());
    let control = directory.join("control.sock");
    let stop = UnixListener::bind(&control).unwrap();
    fs::set_permissions(&control, fs::Permissions::from_mode(0o600)).unwrap();
    let manifest = json!({"scope":"SYNTHETIC BUSINESS CONTROL; ACTUAL PG/AUTH/WIRE SOURCE; NO INFERENCE","directory":directory,"socket":socket,"host_config":auth,"pins_path":pinpath,"prompt_path":prompt,"input_sha256":f.selection_request.input_sha256,"invocation_key":f.selection_request.invocation_key,"workspace_key":c.compare_context.workspace_key,"native_session_id":c.compare_context.native_session_id,"workspace_id":c.workspace,"actor_id":c.owner.principal_id,"session_id":c.sessions[1],"ledger_dir":ledger,"stop_socket":control});
    let manifest_path = directory.join("ready.json");
    private_file(
        &manifest_path,
        serde_json::to_string_pretty(&manifest).unwrap().as_bytes(),
    );
    eprintln!("WIRE_CONTROL_READY {}", manifest_path.display());
    if std::env::var("TECT_TEST_WIRE_HARNESS").as_deref() == Ok("jev-owned-pg18.6-control-only") {
        // Explicit opt-in only. Readiness was actual successful Rust+Python
        // source queries, and no inference has been started by this harness.
        let stopped = tokio::time::timeout(Duration::from_secs(600), stop.accept()).await;
        eprintln!(
            "WIRE_CONTROL_STOP {}",
            if stopped.is_ok() {
                "handshake"
            } else {
                "bounded_timeout"
            }
        );
    }
    server.abort();
    let _ = server.await;
    eprintln!(
        "WIRE_CONTROL_FINISHED S02/S05 current-source wire, exact decoder, denials/RLS and candidate/task lock release passed; synthetic control acceptance only"
    );
}

#[tokio::test]
#[ignore = "requires exact owned PG18.6; real selected-save/S05 cycle control"]
async fn authenticated_host_wire_pg_selected_save_candidate_task_cycle() {
    let f = fixture::build().await;
    let c = &f.control;
    let before = effects(&c.admin_pool, c.owner.tenant_id).await;
    let mut candidate_holder = c.runtime.begin().await.unwrap();
    sqlx::query("SELECT set_config('tect.tenant_id',$1,true)")
        .bind(c.owner.tenant_id.to_string())
        .execute(&mut *candidate_holder)
        .await
        .unwrap();
    sqlx::query("SELECT id FROM slice_candidate_sets WHERE id=$1 FOR SHARE")
        .bind(f.candidate_set)
        .fetch_one(&mut *candidate_holder)
        .await
        .unwrap();
    let holder_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *candidate_holder)
        .await
        .unwrap();
    let mut stale_save = f.save.clone();
    stale_save.request_id = Uuid::new_v4();
    // Deliberately stale: once unblocked this writer must not change source.
    assert_eq!(stale_save.revision, 1);
    let service = f.service.clone();
    let context = c.owner_context.clone();
    let guidance = f.guidance.clone();
    let writer = tokio::spawn(async move {
        service
            .save_slice_candidate_draft(&context, &stale_save, &guidance, &guidance)
            .await
    });
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let waiting: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_stat_activity a WHERE $1=ANY(pg_blocking_pids(a.pid)) AND a.query LIKE '%slice_candidate_sets%' AND a.wait_event_type='Lock')").bind(holder_pid).fetch_one(&c.admin_pool).await.unwrap();
            if waiting { break; }
            tokio::task::yield_now().await;
        }
    }).await.expect("public selected-save did not reach deterministic candidate lock barrier");
    let denied = tokio::time::timeout(
        Duration::from_secs(2),
        f.service
            .prepare_model_route_host_selection(&c.compare_context, &f.selection_request),
    )
    .await
    .unwrap();
    assert!(matches!(
        denied,
        Err(Error::StaleRevision | Error::StaleContext)
    ));
    candidate_holder.commit().await.unwrap();
    let result = tokio::time::timeout(Duration::from_secs(2), writer)
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(
        result,
        Err(Error::StaleRevision | Error::StaleContext)
    ));
    f.service
        .prepare_model_route_host_selection(&c.compare_context, &f.selection_request)
        .await
        .unwrap();
    assert_eq!(effects(&c.admin_pool, c.owner.tenant_id).await, before);
    eprintln!(
        "PUBLIC_SELECTED_SAVE_S05_CYCLE_PASSED candidate SHARE barrier, real writer task lock, prompt S05 abort, stale writer rollback, fresh retry; no source mutation"
    );
}

#[tokio::test]
#[ignore = "requires exact owned PG18.6; swallowed advisory declaration contention"]
async fn authenticated_host_wire_pg_advisory_swallowed_context_abort() {
    let f = fixture::build().await;
    let c = &f.control;
    let before = effects(&c.admin_pool, c.owner.tenant_id).await;
    let store = PgStore::from_pool(c.runtime.clone());
    let mut holder = store.begin(TransactionMode::ReadWrite).await.unwrap();
    let identity = holder.authenticate(&c.owner.auth).await.unwrap();
    holder.set_tenant(c.owner.tenant_id).await.unwrap();
    holder
        .matrix_requirements_context_store()
        .unwrap()
        .lock_matrix_requirements_head(
            c.workspace,
            RequirementsAnchor::Program {
                program_id: c.program,
            },
        )
        .await
        .unwrap();
    let request = RequestEngineeringAdvisory {
        task_id: c.task,
        expected_task_revision: 1,
        request_key: format!("swallowed-abort-control-{}", Uuid::new_v4()),
        session_preference: AdvisoryRequestPreference::UseWorkspace,
        request_preference: AdvisoryRequestPreference::UseWorkspace,
    };
    // The service converts bound-context failure to a no-call reason. The UoW
    // must already be rolled back, so that fallback cannot persist a receipt.
    let losing = tokio::time::timeout(
        Duration::from_secs(2),
        f.service
            .request_engineering_advisory(&c.owner_context, &request),
    )
    .await
    .unwrap();
    assert_eq!(losing, Err(Error::StorageUnavailable));
    assert_eq!(effects(&c.admin_pool, c.owner.tenant_id).await, before);
    let receipt_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM advisory_opportunity WHERE tenant_id=$1 AND request_key=$2",
    )
    .bind(c.owner.tenant_id)
    .bind(&request.request_key)
    .fetch_one(&c.admin_pool)
    .await
    .unwrap();
    assert_eq!(receipt_count, 0);
    // Both the earlier task and native-session locks must be gone before the
    // declaration winner releases its own transaction.
    holder
        .lock_matrix_task(c.workspace, c.task)
        .await
        .unwrap()
        .unwrap();
    tokio::time::timeout(
        Duration::from_secs(2),
        holder.lock_native_session(identity.host_id, &c.owner_context.native_session_id),
    )
    .await
    .unwrap()
    .unwrap();
    holder.commit().await.unwrap();
    let retry = f
        .service
        .request_engineering_advisory(&c.owner_context, &request)
        .await
        .unwrap();
    assert_eq!(retry.state, AdvisoryOpportunityState::Advised);
    assert!(retry.provider_called, "local TEST adapter only");
    eprintln!(
        "PUBLIC_ADVISORY_SWALLOWED_ABORT_PASSED StorageUnavailable, no fallback receipt or effects, earlier task/session locks released, fresh guarded-advice retry; no network"
    );
}
