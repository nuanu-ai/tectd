use super::live_support::{D, manifest, reseal_manifest, rw, set_config};
use super::*;
use crate::{PgStore, admin};
use tect_application::{
    DenyScopeBudget, PreparedScopeAdviceAttempt, ScopeAdviceProvider, ScopeAdviceProviderError,
    ScopeAdviceProviderObservation, ScopeAdviceProviderRequest, SetupFiles, SourceInspector,
    StartedScopeDispatchPermit, WorkspaceService,
};

mod scenarios;
use scenarios::fixtures::*;

#[tokio::test]
#[ignore = "requires disposable PG18 and TECT_TEST_ADMIN_URL/TECT_TEST_RUNTIME_URL/TECT_TEST_RUNTIME_ROLE"]
async fn seven_aggregate_vertical_rejects_wrong_candidate_unresolved_partial_lineage_and_identity()
{
    scenarios::setup::verify!(
        actor,
        advice,
        auth,
        authored_scope_set,
        authority,
        authority_request,
        candidate,
        dispatch,
        enrollment,
        manifest,
        observation,
        observed,
        opportunity,
        pool,
        program,
        reason,
        request_key,
        runtime_pool,
        service,
        session,
        snapshot,
        source_refs,
        state,
        store,
        tenant,
        verifier_session,
        workspace,
    );
    scenarios::positive_provider::verify!(
        advice,
        auth,
        authored_scope_set,
        authority,
        authority_request,
        candidate,
        enrollment,
        opportunity,
        pool,
        replay,
        session,
        started,
        state,
        store,
        tenant,
        workspace,
    );
    scenarios::manifest_validation::verify!(
        altered,
        auth,
        candidate,
        enrollment,
        manifest,
        missing,
        opportunity,
        prepared,
        program,
        rejected,
        replay,
        request_key,
        snapshot,
        source_refs,
        store,
        stored,
        tenant,
        unit,
        workspace,
    );
    scenarios::advice_and_lineage::verify!(
        actor,
        advice,
        answers,
        auth,
        candidate,
        dispatch,
        dispatch_id,
        enrollment,
        first,
        manifest,
        opportunity,
        other_opportunity,
        pool,
        prepared,
        reason,
        replay,
        request,
        result,
        second,
        selected_id,
        session,
        state,
        store,
        tenant,
        third_opportunity,
        unit,
        workspace,
    );
    scenarios::disposition_and_audit::verify!(
        actor,
        advice,
        after,
        audit_query,
        auth,
        candidate,
        context,
        cursor,
        enrollment,
        first,
        foreign,
        foreign_unit,
        manifest,
        observation,
        opportunity,
        other_opportunity,
        page,
        pool,
        reason,
        request,
        result,
        selected_id,
        service,
        session,
        state,
        store,
        tenant,
        third_opportunity,
        unit,
        verifier_session,
        workspace,
    );
    scenarios::dispatch_fences::verify!(
        actor,
        auth,
        candidate,
        dispatch,
        dispatch_id,
        enrollment,
        manifest,
        pool,
        runtime_pool,
        session,
        started,
        state,
        store,
        tenant,
        workspace,
    );
    scenarios::selected_save::verify!(
        actor,
        advice,
        altered,
        answers,
        audit_unit,
        auth,
        authored_scope_set,
        authority,
        authority_request,
        candidate,
        context,
        dispatch_id,
        enrollment,
        failed,
        manifest,
        observation,
        opportunity,
        pool,
        rejected,
        replay,
        request,
        save,
        second,
        second_auth,
        second_store,
        selected_audit,
        selected_id,
        selected_opportunity,
        service,
        session,
        snapshot,
        state,
        store,
        stored,
        tenant,
        unit,
        workspace,
    );
    scenarios::selected_observation::verify!(
        actor,
        after,
        audit_query,
        audit_unit,
        auth,
        candidate,
        context,
        cursor,
        enrollment,
        failed,
        foreign,
        foreign_unit,
        missing,
        observation,
        observed,
        opportunity,
        page,
        pool,
        save,
        second_auth,
        second_store,
        selected_audit,
        selected_opportunity,
        service,
        session,
        store,
        stored,
        tenant,
        unit,
        verifier_session,
        workspace,
    );
}

#[test]
fn authored_request_digest_requires_lowercase_sha256_hex() {
    assert!(super::valid_authored_request_digest(&"a".repeat(64)));
    assert!(!super::valid_authored_request_digest(&"A".repeat(64)));
    assert!(!super::valid_authored_request_digest(&"g".repeat(64)));
    assert!(!super::valid_authored_request_digest(&"a".repeat(63)));
}

#[test]
fn prepared_scope_disposition_uses_only_pre_dispatch_terminal_reasons() {
    assert_eq!(
        super::prepared_scope_disposition_state(AdvisoryReason::DeterministicInputInvalid),
        Ok("no_call")
    );
    assert_eq!(
        super::prepared_scope_disposition_state(AdvisoryReason::ConfigurationChanged),
        Ok("invalidated")
    );
    assert_eq!(
        super::prepared_scope_disposition_state(AdvisoryReason::ProviderUnconfigured),
        Err(Error::InvalidArguments)
    );
    assert!(super::valid_scope_source_digest(&"a".repeat(64)));
    assert!(!super::valid_scope_source_digest(&"A".repeat(64)));
    assert!(!super::valid_scope_source_digest(&"z".repeat(64)));
    assert!(!super::valid_scope_source_digest(&"a".repeat(63)));
}

#[tokio::test]
#[ignore = "requires disposable PG18 and TECT_TEST_ADMIN_URL/TECT_TEST_RUNTIME_URL/TECT_TEST_RUNTIME_ROLE"]
async fn published_source_snapshots_are_permanently_frozen_and_refresh_advances() {
    let admin_url = std::env::var("TECT_TEST_ADMIN_URL").unwrap();
    let runtime_url = std::env::var("TECT_TEST_RUNTIME_URL").unwrap();
    let role = std::env::var("TECT_TEST_RUNTIME_ROLE").unwrap();
    let pool = sqlx::PgPool::connect(&admin_url).await.unwrap();
    admin::migrate(&pool, &role).await.unwrap();
    let runtime = sqlx::PgPool::connect(&runtime_url).await.unwrap();
    let enrollment = admin::enroll_host(&pool, None, vec![]).await.unwrap();
    let tenant = enrollment.tenant_id;
    let workspace = Uuid::new_v4();
    let program = Uuid::new_v4();
    let candidate = Uuid::new_v4();
    let first = Uuid::new_v4();
    let second = Uuid::new_v4();
    sqlx::query("INSERT INTO workspaces(id,tenant_id,key) VALUES($1,$2,$3)")
        .bind(workspace)
        .bind(tenant)
        .bind(format!("source-freeze-{workspace}"))
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO programs(id,tenant_id,workspace_id,status,revision,name,intent,basis,boundaries,constraints,success,current_step,input_cursor,latest_input,max_input_bytes) VALUES($1,$2,$3,'open',1,'p','i','b','finite','c','s','ready',0,1,4096)")
        .bind(program).bind(tenant).bind(workspace).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO scope_candidate_sets(id,tenant_id,workspace_id,program_id,origin_request_id,origin_input,origin_payload,status,boundary,max_input_bytes) VALUES($1,$2,$3,$4,$5,'input','{}','ready','finite',4096)")
        .bind(candidate).bind(tenant).bind(workspace).bind(program).bind(Uuid::new_v4())
        .execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO scope_candidate_contents(tenant_id,workspace_id,digest,body) VALUES($1,$2,$3,'body')")
        .bind(tenant).bind(workspace).bind(D).execute(&pool).await.unwrap();
    for (id, sequence) in [(first, 1_i64), (second, 2)] {
        sqlx::query("INSERT INTO scope_candidate_snapshots(id,tenant_id,workspace_id,candidate_set_id,sequence,program_revision,program_latest_input,planning_latest_input,program_body_digest,selected_worktree_ids,selected_sources_digest,method_id,method_revision,method_digest,method_body,method_origin_refs,registry_revision,registry_digest,rules) VALUES($1,$2,$3,$4,$5,1,1,1,$6,'{}',$6,'m','1',$6,'body','[]','1',$6,'[]')")
            .bind(id).bind(tenant).bind(workspace).bind(candidate).bind(sequence).bind(D)
            .execute(&pool).await.unwrap();
    }

    // Exercise the trigger through the actual runtime role and tenant policy.
    let mut runtime_tx = runtime.begin().await.unwrap();
    sqlx::query("SELECT set_config('tect.tenant_id',$1,true)")
        .bind(tenant.to_string())
        .execute(&mut *runtime_tx)
        .await
        .unwrap();
    sqlx::query("INSERT INTO scope_candidate_source_refs(tenant_id,workspace_id,candidate_set_id,snapshot_id,kind,program_field,body_digest,label) VALUES($1,$2,$3,$4,'program_field','intent',$5,'intent')")
        .bind(tenant).bind(workspace).bind(candidate).bind(first).bind(D)
        .execute(&mut *runtime_tx).await.unwrap();
    runtime_tx.commit().await.unwrap();
    sqlx::query("UPDATE scope_candidate_sets SET current_snapshot_id=$1 WHERE id=$2")
        .bind(first)
        .bind(candidate)
        .execute(&pool)
        .await
        .unwrap();
    let late = sqlx::query("INSERT INTO scope_candidate_source_refs(tenant_id,workspace_id,candidate_set_id,snapshot_id,kind,program_field,body_digest,label) VALUES($1,$2,$3,$4,'program_field','basis',$5,'basis')")
        .bind(tenant).bind(workspace).bind(candidate).bind(first).bind(D)
        .execute(&pool).await;
    assert!(
        late.is_err(),
        "published snapshot accepted a late reference"
    );

    // A pending refresh can accumulate refs, then publish at a greater sequence.
    sqlx::query("INSERT INTO scope_candidate_source_refs(tenant_id,workspace_id,candidate_set_id,snapshot_id,kind,program_field,body_digest,label) VALUES($1,$2,$3,$4,'program_field','basis',$5,'basis')")
        .bind(tenant).bind(workspace).bind(candidate).bind(second).bind(D)
        .execute(&pool).await.unwrap();
    sqlx::query("UPDATE scope_candidate_sets SET current_snapshot_id=$1 WHERE id=$2")
        .bind(second)
        .bind(candidate)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE scope_candidate_sets SET current_snapshot_id=$1,revision=revision+1 WHERE id=$2",
    )
    .bind(second)
    .bind(candidate)
    .execute(&pool)
    .await
    .unwrap();
    assert!(
        sqlx::query("UPDATE scope_candidate_sets SET current_snapshot_id=$1 WHERE id=$2")
            .bind(first)
            .bind(candidate)
            .execute(&pool)
            .await
            .is_err()
    );
    assert!(
        sqlx::query("UPDATE scope_candidate_sets SET current_snapshot_id=NULL WHERE id=$1")
            .bind(candidate)
            .execute(&pool)
            .await
            .is_err()
    );
    assert!(sqlx::query("INSERT INTO scope_candidate_source_refs(tenant_id,workspace_id,candidate_set_id,snapshot_id,kind,program_field,body_digest,label) VALUES($1,$2,$3,$4,'program_field','constraints',$5,'constraints')")
        .bind(tenant).bind(workspace).bind(candidate).bind(second).bind(D)
        .execute(&pool).await.is_err());

    // Publication holds the same set-row lock as manifest preparation.
    let third = Uuid::new_v4();
    sqlx::query("INSERT INTO scope_candidate_snapshots(id,tenant_id,workspace_id,candidate_set_id,sequence,program_revision,program_latest_input,planning_latest_input,program_body_digest,selected_worktree_ids,selected_sources_digest,method_id,method_revision,method_digest,method_body,method_origin_refs,registry_revision,registry_digest,rules) VALUES($1,$2,$3,$4,3,1,1,1,$5,'{}',$5,'m','1',$5,'body','[]','1',$5,'[]')")
        .bind(third).bind(tenant).bind(workspace).bind(candidate).bind(D)
        .execute(&pool).await.unwrap();
    let mut publisher = pool.begin().await.unwrap();
    sqlx::query("UPDATE scope_candidate_sets SET current_snapshot_id=$1 WHERE id=$2")
        .bind(third)
        .bind(candidate)
        .execute(&mut *publisher)
        .await
        .unwrap();
    let insert_pool = pool.clone();
    let insert = tokio::spawn(async move {
        sqlx::query("INSERT INTO scope_candidate_source_refs(tenant_id,workspace_id,candidate_set_id,snapshot_id,kind,program_field,body_digest,label) VALUES($1,$2,$3,$4,'program_field','boundaries',$5,'boundaries')")
            .bind(tenant).bind(workspace).bind(candidate).bind(third).bind(D)
            .execute(&insert_pool).await
    });
    tokio::pin!(insert);
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(150), &mut insert)
            .await
            .is_err(),
        "insert did not wait for publication lock"
    );
    publisher.commit().await.unwrap();
    assert!(
        insert.await.unwrap().is_err(),
        "concurrent append escaped publication freeze"
    );
}
