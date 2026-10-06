//! Real PgStore pages over authorized native planning fixtures.
#[allow(dead_code)]
mod recovery_support;
#[path = "native_planning/support.rs"]
#[allow(dead_code)]
mod support;
use recovery_support::{Daemon, Mcp, host_file, private_temp, tagged_url};
use serde_json::json;
use sqlx::PgPool;
use std::{collections::BTreeSet, sync::Arc};
use tect_application::WorkspaceService;
use tect_domain::{Error, RequestContext, WorkspaceCollection, WorkspaceCollectionCursor};
use tect_postgres::{PgStore, admin};
use uuid::Uuid;

#[path = "workspace_collection_pagination/support.rs"]
mod collection_support;
use collection_support::{ordered_ids, tied_pages};

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn native_pgstore_collection_pages_are_complete_scoped_and_stable() {
    let admin_url = std::env::var("TECT_TEST_ADMIN_URL").expect("TECT_TEST_ADMIN_URL required");
    let runtime_url =
        std::env::var("TECT_TEST_RUNTIME_URL").expect("TECT_TEST_RUNTIME_URL required");
    let role = std::env::var("TECT_TEST_RUNTIME_ROLE").expect("TECT_TEST_RUNTIME_ROLE required");
    let pool = PgPool::connect(&admin_url).await.unwrap();
    admin::migrate(&pool, &role).await.unwrap();
    let temp = private_temp();
    let root = temp.path().canonicalize().unwrap();
    let repo = root.join("source");
    support::repository(&repo);
    let runtime = tagged_url(
        &runtime_url,
        &format!("tect-collection-pages-{}", Uuid::new_v4()),
    );
    let socket = root.join("pages.sock");
    let _daemon = Daemon::start(&runtime, socket.clone()).await;
    let host = admin::enroll_host(&pool, None, vec![root.to_string_lossy().into_owned()])
        .await
        .unwrap();
    let config = root.join("host.json");
    host_file(&config, &host.auth);
    let context = RequestContext {
        auth: host.auth.clone(),
        native_session_id: Uuid::new_v4().to_string(),
        workspace_key: format!("pages-{}", Uuid::new_v4()),
    };
    let mut client = Mcp::start(
        &socket,
        &config,
        &context.native_session_id,
        &context.workspace_key,
    )
    .await;
    let mut candidate_ids = Vec::new();
    let mut scope_ids = Vec::new();
    for _ in 0..28 {
        let (source, candidate) = support::ready_source_candidate(&mut client, &repo).await;
        candidate_ids.push(support::id(&source["candidate_set"]["id"]));
        let mutation = support::route(&mut client, "command", "scope.open", json!({
            "request_id":Uuid::new_v4(),"candidate_set_id":source["candidate_set"]["id"],
            "candidate_set_revision":source["candidate_set"]["revision"],"candidate_snapshot_id":source["snapshot"]["id"],
            "candidate_id":candidate["id"],"candidate_revision":candidate["revision"]
        })).await;
        scope_ids.push(support::id(&mutation["scope"]["id"]));
    }
    let service = WorkspaceService::new(
        Arc::new(PgStore::connect(&runtime, 4).await.unwrap()),
        Arc::new(tect_host::GitSourceInspector),
        Arc::new(tect_host::LocalSetupFiles),
    );
    let state = service.get_state(&context).await.unwrap();
    let workspace = state.workspace.as_ref().unwrap().id;
    candidate_ids = ordered_ids(&pool, "scope_candidate_sets", host.tenant_id, workspace).await;
    scope_ids = ordered_ids(&pool, "native_scopes", host.tenant_id, workspace).await;
    let first = service.get_state(&context).await.unwrap();
    assert_eq!(
        first
            .candidate_sets
            .iter()
            .map(|s| s.id)
            .collect::<Vec<_>>(),
        candidate_ids[..25]
    );
    assert_eq!(
        first
            .native_planning
            .iter()
            .map(|s| s.scope_id)
            .collect::<Vec<_>>(),
        scope_ids[..25]
    );
    assert!(first.candidate_sets_next_after.is_some());
    assert!(first.native_planning_next_after.is_some());
    let mut candidates = Vec::new();
    let mut native = Vec::new();
    let mut after = None::<String>;
    loop {
        let (_, _, page) = service
            .read_candidate_sets_bound(&context, after.as_deref(), 7)
            .await
            .unwrap();
        assert!(page.candidate_sets.len() <= 7);
        candidates.extend(page.candidate_sets.iter().map(|s| s.id));
        after = page.next_after.map(|c| c.encode());
        if after.is_none() {
            break;
        }
    }
    let mut after = None::<String>;
    loop {
        let (_, _, page) = service
            .read_native_planning_bound(&context, after.as_deref(), 7)
            .await
            .unwrap();
        assert!(page.native_planning.len() <= 7);
        native.extend(page.native_planning.iter().map(|s| s.scope_id));
        after = page.next_after.map(|c| c.encode());
        if after.is_none() {
            break;
        }
    }
    assert_eq!(candidates, candidate_ids);
    assert_eq!(native, scope_ids);
    assert_eq!(candidates.iter().collect::<BTreeSet<_>>().len(), 28);
    assert_eq!(native.iter().collect::<BTreeSet<_>>().len(), 28);
    tied_pages(
        &pool,
        &service,
        &socket,
        &config,
        &host.auth,
        host.tenant_id,
        &repo,
        workspace,
    )
    .await;
    for limit in [0, 26] {
        assert_eq!(
            service
                .read_candidate_sets_bound(&context, None, limit)
                .await,
            Err(Error::InvalidArguments)
        );
        assert_eq!(
            service
                .read_native_planning_bound(&context, None, limit)
                .await,
            Err(Error::InvalidArguments)
        );
    }
    let cursor = |collection, workspace_id, anchor_id| {
        WorkspaceCollectionCursor {
            workspace_id,
            collection,
            anchor_id,
        }
        .encode()
    };
    for bad in [
        cursor(
            WorkspaceCollection::NativePlanning,
            workspace,
            candidate_ids[0],
        ),
        cursor(
            WorkspaceCollection::CandidateSets,
            Uuid::new_v4(),
            candidate_ids[0],
        ),
        cursor(WorkspaceCollection::CandidateSets, workspace, scope_ids[0]),
        cursor(
            WorkspaceCollection::CandidateSets,
            workspace,
            Uuid::new_v4(),
        ),
    ] {
        assert_eq!(
            service
                .read_candidate_sets_bound(&context, Some(&bad), 7)
                .await,
            Err(Error::InvalidArguments)
        );
    }
    for bad in [
        cursor(WorkspaceCollection::CandidateSets, workspace, scope_ids[0]),
        cursor(
            WorkspaceCollection::NativePlanning,
            Uuid::new_v4(),
            scope_ids[0],
        ),
        cursor(
            WorkspaceCollection::NativePlanning,
            workspace,
            candidate_ids[0],
        ),
        cursor(
            WorkspaceCollection::NativePlanning,
            workspace,
            Uuid::new_v4(),
        ),
    ] {
        assert_eq!(
            service
                .read_native_planning_bound(&context, Some(&bad), 7)
                .await,
            Err(Error::InvalidArguments)
        );
    }
    // A valid anchor from another tenant remains invalid even when its cursor claims this workspace.
    let foreign = admin::enroll_host(&pool, None, Vec::new()).await.unwrap();
    let foreign_context = RequestContext {
        auth: foreign.auth,
        native_session_id: Uuid::new_v4().to_string(),
        workspace_key: "foreign-pages".into(),
    };
    service.open_workspace(&foreign_context).await.unwrap();
    for bad in [cursor(
        WorkspaceCollection::CandidateSets,
        service
            .get_state(&foreign_context)
            .await
            .unwrap()
            .workspace
            .unwrap()
            .id,
        candidate_ids[0],
    )] {
        assert_eq!(
            service
                .read_candidate_sets_bound(&foreign_context, Some(&bad), 7)
                .await,
            Err(Error::InvalidArguments)
        );
    }
    let foreign_workspace = service
        .get_state(&foreign_context)
        .await
        .unwrap()
        .workspace
        .unwrap()
        .id;
    assert_eq!(
        service
            .read_native_planning_bound(
                &foreign_context,
                Some(&cursor(
                    WorkspaceCollection::NativePlanning,
                    foreign_workspace,
                    scope_ids[0]
                )),
                7
            )
            .await,
        Err(Error::InvalidArguments)
    );
    let other_context = RequestContext {
        auth: host.auth.clone(),
        native_session_id: Uuid::new_v4().to_string(),
        workspace_key: "other-pages".into(),
    };
    let other_workspace = service
        .open_workspace(&other_context)
        .await
        .unwrap()
        .workspace
        .unwrap()
        .id;
    assert_eq!(
        service
            .read_candidate_sets_bound(
                &other_context,
                Some(&cursor(
                    WorkspaceCollection::CandidateSets,
                    other_workspace,
                    candidate_ids[0]
                )),
                7
            )
            .await,
        Err(Error::InvalidArguments)
    );
    assert_eq!(
        service
            .read_native_planning_bound(
                &other_context,
                Some(&cursor(
                    WorkspaceCollection::NativePlanning,
                    other_workspace,
                    scope_ids[0]
                )),
                7
            )
            .await,
        Err(Error::InvalidArguments)
    );
    // Delete fresh, unreferenced anchors; continuation must refuse rather than restart.
    let deleted_candidate = Uuid::new_v4();
    let deleted_program = Uuid::new_v4();
    sqlx::query("INSERT INTO programs(id,tenant_id,workspace_id,status,revision,current_step,input_cursor,latest_input,max_input_bytes) VALUES($1,$2,$3,'draft',1,'compose',0,1,0)")
        .bind(deleted_program).bind(host.tenant_id).bind(workspace).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO scope_candidate_sets(id,tenant_id,workspace_id,program_id,origin_request_id,origin_input,origin_payload,boundary,max_input_bytes) VALUES($1,$2,$3,$4,$5,'fixture','{}','finite',0)")
        .bind(deleted_candidate).bind(host.tenant_id).bind(workspace).bind(deleted_program).bind(Uuid::new_v4()).execute(&pool).await.unwrap();
    sqlx::query(
        "DELETE FROM scope_candidate_sets WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3",
    )
    .bind(host.tenant_id)
    .bind(workspace)
    .bind(deleted_candidate)
    .execute(&pool)
    .await
    .unwrap();
    assert_eq!(
        service
            .read_candidate_sets_bound(
                &context,
                Some(&cursor(
                    WorkspaceCollection::CandidateSets,
                    workspace,
                    deleted_candidate
                )),
                7
            )
            .await,
        Err(Error::InvalidArguments)
    );
    let deleted_scope = Uuid::new_v4();
    sqlx::query("INSERT INTO native_scopes(id,tenant_id,workspace_id,source_candidate_set_id,source_candidate_set_revision,source_snapshot_id,source_candidate_id,source_candidate_revision,boundary,title,outcome,includes,excludes,origin_request_id,origin_payload) SELECT $1,tenant_id,workspace_id,source_candidate_set_id,1,source_snapshot_id,$4,1,'finite','fixture','fixture','[]','[]',$5,'{}' FROM native_scopes WHERE tenant_id=$2 AND workspace_id=$3 LIMIT 1")
        .bind(deleted_scope).bind(host.tenant_id).bind(workspace).bind(Uuid::new_v4()).bind(Uuid::new_v4()).execute(&pool).await.unwrap();
    sqlx::query("DELETE FROM native_scopes WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3")
        .bind(host.tenant_id)
        .bind(workspace)
        .bind(deleted_scope)
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        service
            .read_native_planning_bound(
                &context,
                Some(&cursor(
                    WorkspaceCollection::NativePlanning,
                    workspace,
                    deleted_scope
                )),
                7
            )
            .await,
        Err(Error::InvalidArguments)
    );
}
