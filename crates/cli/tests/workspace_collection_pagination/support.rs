use super::*;

// Read timestamp metadata without SQL ordering; derive the expected order independently.
pub(super) async fn ordered_ids(
    pool: &PgPool,
    table: &str,
    tenant: Uuid,
    workspace: Uuid,
) -> Vec<Uuid> {
    let mut rows: Vec<(Uuid, String)> = sqlx::query_as(&format!("SELECT id,to_char(created_at AT TIME ZONE 'UTC','YYYY-MM-DD HH24:MI:SS.US') FROM {table} WHERE tenant_id=$1 AND workspace_id=$2"))
        .bind(tenant).bind(workspace).fetch_all(pool).await.unwrap();
    rows.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    rows.into_iter().map(|row| row.0).collect()
}

pub(super) async fn row_versions(
    pool: &PgPool,
    table: &str,
    tenant: Uuid,
    workspace: Uuid,
) -> Vec<String> {
    sqlx::query_scalar(&format!("SELECT xmin::text || ':' || row_to_json(t)::text FROM {table} t WHERE tenant_id=$1 AND workspace_id=$2 ORDER BY id"))
        .bind(tenant).bind(workspace).fetch_all(pool).await.unwrap()
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn tied_pages(
    pool: &PgPool,
    service: &WorkspaceService,
    socket: &std::path::Path,
    config: &std::path::Path,
    auth: &tect_domain::HostAuth,
    tenant: Uuid,
    repo: &std::path::Path,
    producer_workspace: Uuid,
) {
    let context = RequestContext {
        auth: auth.clone(),
        native_session_id: Uuid::new_v4().to_string(),
        workspace_key: format!("candidate-ties-{}", Uuid::new_v4()),
    };
    let state = service.open_workspace(&context).await.unwrap();
    let workspace = state.workspace.unwrap().id;
    let session = state.session.unwrap().id;
    let mut client = Mcp::start(
        socket,
        config,
        &context.native_session_id,
        &context.workspace_key,
    )
    .await;
    let mut ids = [Uuid::new_v4(), Uuid::new_v4()];
    ids.sort();
    for id in ids.into_iter().rev() {
        let program = support::ready_program(&mut client, repo).await;
        insert_tied_candidate(
            pool,
            tenant,
            workspace,
            session,
            id,
            &program,
            producer_workspace,
        )
        .await;
    }
    assert_timestamps(pool, "scope_candidate_sets", tenant, workspace).await;
    let before = row_versions(pool, "scope_candidate_sets", tenant, workspace).await;
    let (_, _, first) = service
        .read_candidate_sets_bound(&context, None, 1)
        .await
        .unwrap();
    assert_eq!(
        first
            .candidate_sets
            .iter()
            .map(|r| r.id)
            .collect::<Vec<_>>(),
        [ids[0]]
    );
    let cursor = first.next_after.unwrap();
    assert_eq!(cursor.anchor_id, ids[0]);
    let (_, _, second) = service
        .read_candidate_sets_bound(&context, Some(&cursor.encode()), 1)
        .await
        .unwrap();
    assert_eq!(
        second
            .candidate_sets
            .iter()
            .map(|r| r.id)
            .collect::<Vec<_>>(),
        [ids[1]]
    );
    assert!(second.next_after.is_none());
    assert_eq!(
        before,
        row_versions(pool, "scope_candidate_sets", tenant, workspace).await
    );
    client.finish().await;

    let context = RequestContext {
        auth: auth.clone(),
        native_session_id: Uuid::new_v4().to_string(),
        workspace_key: format!("native-ties-{}", Uuid::new_v4()),
    };
    let workspace = service
        .open_workspace(&context)
        .await
        .unwrap()
        .workspace
        .unwrap()
        .id;
    let mut client = Mcp::start(
        socket,
        config,
        &context.native_session_id,
        &context.workspace_key,
    )
    .await;
    let mut ids = [Uuid::new_v4(), Uuid::new_v4()];
    ids.sort();
    for id in ids.into_iter().rev() {
        let (source, candidate) = support::ready_source_candidate(&mut client, repo).await;
        insert_tied_native(
            pool,
            tenant,
            workspace,
            id,
            &source,
            &candidate,
            producer_workspace,
        )
        .await;
    }
    assert_timestamps(pool, "native_scopes", tenant, workspace).await;
    let before = row_versions(pool, "native_scopes", tenant, workspace).await;
    let (_, _, first) = service
        .read_native_planning_bound(&context, None, 1)
        .await
        .unwrap();
    assert_eq!(
        first
            .native_planning
            .iter()
            .map(|r| r.scope_id)
            .collect::<Vec<_>>(),
        [ids[0]]
    );
    let cursor = first.next_after.unwrap();
    assert_eq!(cursor.anchor_id, ids[0]);
    let (_, _, second) = service
        .read_native_planning_bound(&context, Some(&cursor.encode()), 1)
        .await
        .unwrap();
    assert_eq!(
        second
            .native_planning
            .iter()
            .map(|r| r.scope_id)
            .collect::<Vec<_>>(),
        [ids[1]]
    );
    assert!(second.next_after.is_none());
    for summary in first.native_planning.iter().chain(&second.native_planning) {
        assert_eq!(summary.candidate_set_revision, 1);
        assert_eq!(
            summary.candidate_set_status,
            tect_domain::SliceCandidateSetStatus::Draft
        );
        assert!(!summary.snapshot_id.is_nil());
        assert!(!summary.stale);
    }
    assert_eq!(
        before,
        row_versions(pool, "native_scopes", tenant, workspace).await
    );
    client.finish().await;
}

pub(super) async fn assert_timestamps(pool: &PgPool, table: &str, tenant: Uuid, workspace: Uuid) {
    let stamps:Vec<String>=sqlx::query_scalar(&format!("SELECT to_char(created_at AT TIME ZONE 'UTC','YYYY-MM-DD HH24:MI:SS.US') FROM {table} WHERE tenant_id=$1 AND workspace_id=$2"))
        .bind(tenant).bind(workspace).fetch_all(pool).await.unwrap();
    assert_eq!(stamps, vec!["2026-01-01 00:00:00.000000"; 2]);
}

// Exact begin.rs + snapshot.rs initial structure; only head INSERT adds explicit created_at.
#[allow(clippy::too_many_arguments)]
async fn insert_tied_candidate(
    pool: &PgPool,
    tenant: Uuid,
    workspace: Uuid,
    session: Uuid,
    id: Uuid,
    program: &serde_json::Value,
    producer_workspace: Uuid,
) {
    use sha2::{Digest, Sha256};
    let mut tx = pool.begin().await.unwrap();
    let program_id = support::id(&program["id"]);
    let request = Uuid::new_v4();
    let snapshot = Uuid::new_v4();
    let input = "Open one native Scope for diagnosis and its result-driven correction decision.";
    let payload = json!({"request_id":request,"program_id":program_id,"program_revision":program["revision"],"boundary":"ongoing","input":input});
    sqlx::query("INSERT INTO scope_candidate_sets(id,tenant_id,workspace_id,program_id,origin_request_id,origin_input,origin_payload,boundary,max_input_bytes,created_at) VALUES($1,$2,$3,$4,$5,$6,$7,'ongoing',$8,'2026-01-01T00:00:00Z')")
        .bind(id).bind(tenant).bind(workspace).bind(program_id).bind(request).bind(input).bind(payload).bind(input.len() as i64).execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO scope_candidate_inputs(tenant_id,workspace_id,candidate_set_id,sequence,request_id,session_id,input) VALUES($1,$2,$3,1,$4,$5,$6)")
        .bind(tenant).bind(workspace).bind(id).bind(request).bind(session).bind(input).execute(&mut *tx).await.unwrap();
    let body = serde_json::to_string(program).unwrap();
    let digest = format!("{:x}", Sha256::digest(body.as_bytes()));
    sqlx::query("INSERT INTO scope_candidate_contents(tenant_id,workspace_id,digest,body) VALUES($1,$2,$3,$4) ON CONFLICT DO NOTHING")
        .bind(tenant).bind(workspace).bind(&digest).bind(&body).execute(&mut *tx).await.unwrap();
    let selected: Vec<(Uuid, Uuid)> = sqlx::query_as("SELECT w.repository_id,w.id FROM session_worktrees s JOIN source_worktrees w ON w.tenant_id=s.tenant_id AND w.workspace_id=s.workspace_id AND w.host_id=s.host_id AND w.id=s.worktree_id WHERE s.tenant_id=$1 AND s.workspace_id=$2 AND s.session_id=$3 ORDER BY w.id")
        .bind(tenant).bind(workspace).bind(session).fetch_all(&mut *tx).await.unwrap();
    let selected_ids = selected.iter().map(|row| row.1).collect::<Vec<_>>();
    let selected_digest = format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&selected).unwrap())
    );
    let checked_template = verify_producer_guidance(
        &mut tx,
        "scope_candidate_snapshots",
        tenant,
        producer_workspace,
    )
    .await;
    // Immutable method/registry/rules are taken from an ordinary actual producer snapshot.
    sqlx::query("INSERT INTO scope_candidate_snapshots(id,tenant_id,workspace_id,candidate_set_id,sequence,program_revision,program_latest_input,planning_latest_input,program_body_digest,selected_worktree_ids,selected_sources_digest,method_id,method_revision,method_digest,method_body,method_origin_refs,registry_revision,registry_digest,rules) SELECT $1,$2,$3,$4,1,$5,$6,1,$7,$8,$10,method_id,method_revision,method_digest,method_body,method_origin_refs,registry_revision,registry_digest,rules FROM scope_candidate_snapshots WHERE tenant_id=$2 AND workspace_id=$9 AND id=$11")
        .bind(snapshot).bind(tenant).bind(workspace).bind(id).bind(program["revision"].as_i64().unwrap()).bind(program["latest_input"].as_i64().unwrap()).bind(&digest)
        .bind(selected_ids).bind(producer_workspace).bind(selected_digest).bind(checked_template).execute(&mut *tx).await.unwrap();
    for (kind, sequence, field, text, label) in [
        ("name", program["name"].as_str()),
        ("intent", program["intent"].as_str()),
        ("basis", program["basis"].as_str()),
        ("boundaries", program["boundaries"].as_str()),
        ("constraints", program["constraints"].as_str()),
        ("success", program["success"].as_str()),
    ]
    .into_iter()
    .filter_map(|(field, text)| {
        text.map(|text| {
            (
                if field == "success" {
                    "program_success"
                } else {
                    "program_field"
                },
                None::<i64>,
                Some(field),
                text,
                format!("Captured Program {field}"),
            )
        })
    })
    .chain(std::iter::once((
        "planning_input",
        Some(1),
        None,
        input,
        "Planning request input 1".into(),
    ))) {
        let digest = format!("{:x}", Sha256::digest(text.as_bytes()));
        sqlx::query("INSERT INTO scope_candidate_contents(tenant_id,workspace_id,digest,body) VALUES($1,$2,$3,$4) ON CONFLICT DO NOTHING").bind(tenant).bind(workspace).bind(&digest).bind(text).execute(&mut *tx).await.unwrap();
        sqlx::query("INSERT INTO scope_candidate_source_refs(tenant_id,workspace_id,candidate_set_id,snapshot_id,kind,input_sequence,program_field,body_digest,label) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9)").bind(tenant).bind(workspace).bind(id).bind(snapshot).bind(kind).bind(sequence).bind(field).bind(digest).bind(label).execute(&mut *tx).await.unwrap();
    }
    sqlx::query("UPDATE scope_candidate_sets SET current_snapshot_id=$4 WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3").bind(tenant).bind(workspace).bind(id).bind(snapshot).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
}

// Exact scope.rs ordinary fresh Scope graph, bound to two actual unused accepted candidates.
async fn insert_tied_native(
    pool: &PgPool,
    tenant: Uuid,
    workspace: Uuid,
    scope: Uuid,
    source: &serde_json::Value,
    candidate: &serde_json::Value,
    producer_workspace: Uuid,
) {
    let mut tx = pool.begin().await.unwrap();
    let set = Uuid::new_v4();
    let snapshot = Uuid::new_v4();
    let request = Uuid::new_v4();
    let payload = json!({"request_id":request,"candidate_set_id":source["candidate_set"]["id"],"candidate_set_revision":source["candidate_set"]["revision"],"candidate_snapshot_id":source["snapshot"]["id"],"candidate_id":candidate["id"],"candidate_revision":candidate["revision"]});
    sqlx::query("INSERT INTO native_scopes(id,tenant_id,workspace_id,source_candidate_set_id,source_candidate_set_revision,source_snapshot_id,source_candidate_id,source_candidate_revision,boundary,title,outcome,includes,excludes,origin_request_id,origin_payload,created_at) VALUES($1,$2,$3,$4,$5,$6,$7,$8,'ongoing',$9,$10,$11,$12,$13,$14,'2026-01-01T00:00:00Z')")
        .bind(scope).bind(tenant).bind(workspace).bind(support::id(&source["candidate_set"]["id"])).bind(source["candidate_set"]["revision"].as_i64().unwrap()).bind(support::id(&source["snapshot"]["id"])).bind(support::id(&candidate["id"])).bind(candidate["revision"].as_i64().unwrap()).bind(candidate["title"].as_str().unwrap()).bind(candidate["outcome"].as_str().unwrap()).bind(&candidate["includes"]).bind(&candidate["excludes"]).bind(request).bind(payload).execute(&mut *tx).await.unwrap();
    sqlx::query(
        "INSERT INTO slice_candidate_sets(id,tenant_id,workspace_id,scope_id) VALUES($1,$2,$3,$4)",
    )
    .bind(set)
    .bind(tenant)
    .bind(workspace)
    .bind(scope)
    .execute(&mut *tx)
    .await
    .unwrap();
    let checked_template = verify_producer_guidance(
        &mut tx,
        "slice_planning_snapshots",
        tenant,
        producer_workspace,
    )
    .await;
    sqlx::query("INSERT INTO slice_planning_snapshots(id,tenant_id,workspace_id,candidate_set_id,sequence,scope_revision,source_candidate_set_revision,source_snapshot_id,planning_latest_input,method,registry_revision,registry_digest,rules,catalogue,result_ids) SELECT $1,$2,$3,$4,1,1,$5,$6,0,method,registry_revision,registry_digest,rules,catalogue,'{}'::uuid[] FROM slice_planning_snapshots WHERE tenant_id=$2 AND workspace_id=$7 AND id=$8")
        .bind(snapshot).bind(tenant).bind(workspace).bind(set).bind(source["candidate_set"]["revision"].as_i64().unwrap()).bind(support::id(&source["snapshot"]["id"])).bind(producer_workspace).bind(checked_template).execute(&mut *tx).await.unwrap();
    sqlx::query("UPDATE slice_candidate_sets SET current_snapshot_id=$4 WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3").bind(tenant).bind(workspace).bind(set).bind(snapshot).execute(&mut *tx).await.unwrap();
    sqlx::query("UPDATE native_scopes SET slice_candidate_set_id=$4 WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3").bind(tenant).bind(workspace).bind(scope).bind(set).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
}

// Templates were produced earlier in this same native test by this exact daemon build.
// Only immutable guidance fields are reused; these assertions reject live entity references.
// Return the checked snapshot UUID so each INSERT SELECT copies that exact inspected row.
async fn verify_producer_guidance(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    table: &str,
    tenant: Uuid,
    workspace: Uuid,
) -> Uuid {
    let fields = if table == "scope_candidate_snapshots" {
        "jsonb_build_object('method_id',method_id,'method_revision',method_revision,'method_digest',method_digest,'method_body',method_body,'method_origin_refs',method_origin_refs,'registry_revision',registry_revision,'registry_digest',registry_digest,'rules',rules)"
    } else {
        "jsonb_build_object('method',method,'registry_revision',registry_revision,'registry_digest',registry_digest,'rules',rules,'catalogue',catalogue)"
    };
    let (snapshot, guidance): (Uuid, serde_json::Value) = sqlx::query_as(&format!(
        "SELECT id,{fields} FROM {table} WHERE tenant_id=$1 AND workspace_id=$2 LIMIT 1"
    ))
    .bind(tenant)
    .bind(workspace)
    .fetch_one(&mut **tx)
    .await
    .unwrap();
    let serialized = serde_json::to_string(&guidance).unwrap();
    for id in [tenant, workspace, snapshot] {
        assert!(
            !serialized.contains(&id.to_string()),
            "guidance references live template identity {id}"
        );
    }
    let requests: Vec<Uuid> = sqlx::query_scalar("SELECT origin_request_id FROM scope_candidate_sets WHERE tenant_id=$1 AND workspace_id=$2 UNION ALL SELECT origin_request_id FROM native_scopes WHERE tenant_id=$1 AND workspace_id=$2 UNION ALL SELECT request_id FROM program_inputs WHERE tenant_id=$1 AND workspace_id=$2")
        .bind(tenant).bind(workspace).fetch_all(&mut **tx).await.unwrap();
    for id in requests {
        assert!(
            !serialized.contains(&id.to_string()),
            "guidance references template request {id}"
        );
    }
    for entity_table in [
        "programs",
        "scope_candidate_sets",
        "scope_candidate_snapshots",
        "scope_candidate_inputs",
        "native_scopes",
        "slice_candidate_sets",
        "slice_planning_snapshots",
    ] {
        let ids: Vec<Uuid> = sqlx::query_scalar(&format!(
            "SELECT id FROM {entity_table} WHERE tenant_id=$1 AND workspace_id=$2"
        ))
        .bind(tenant)
        .bind(workspace)
        .fetch_all(&mut **tx)
        .await
        .unwrap();
        for id in ids {
            assert!(
                !serialized.contains(&id.to_string()),
                "guidance references template entity {id}"
            );
        }
    }
    snapshot
}
