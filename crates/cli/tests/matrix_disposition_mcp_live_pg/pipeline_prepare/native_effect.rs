//! Native advice is not caller authority or verification evidence.
use super::*;

pub(super) async fn exercise(
    fixture: &NoCallFixture<'_>,
    socket: &std::path::Path,
    owner: &mut Mcp,
    prepared: &Value,
    ranked: &Value,
    ready: &Value,
) {
    let pool = fixture.pool;
    let workspace = fixture.workspace;
    let scope = Uuid::parse_str(ready["scope"]["id"].as_str().unwrap()).unwrap();
    let counts:(i64,i64,i64)=sqlx::query_as("SELECT (SELECT count(*) FROM native_slices WHERE workspace_id=$1),(SELECT count(*) FROM slice_pipeline_runs WHERE workspace_id=$1),(SELECT count(*) FROM slice_pipeline_phase_attempts WHERE workspace_id=$1)").bind(workspace).fetch_one(pool).await.unwrap();
    assert_eq!(counts, (0, 0, 0), "native ranking cannot execute anything");
    let disposition = route(owner,"command","pipeline.recommendation.disposition",json!({
        "request_id":Uuid::new_v4(),"opportunity_id":prepared["opportunity_id"],
        "expected_work_revision":fixture.work["revision"],"manifest_digest":prepared["manifest_digest"],
        "action":"accept_recommendation","rationale":"Explicitly accept eligible native fixture advice."})).await;
    assert_eq!(disposition["selected_option_id"], ranked["ranked_ids"][0]);
    let no_slice: i64 =
        sqlx::query_scalar("SELECT count(*) FROM native_slices WHERE workspace_id=$1")
            .bind(workspace)
            .fetch_one(pool)
            .await
            .unwrap();
    assert_eq!(no_slice, 0, "disposition alone cannot open a Slice");
    let opened = route(
        owner,
        "command",
        "slice.open",
        json!({
        "request_id":Uuid::new_v4(),"scope_id":scope,"scope_revision":ready["scope"]["revision"],
        "candidate_set_id":fixture.set,"candidate_set_revision":ready["candidate_set"]["revision"],
        "candidate_snapshot_id":ready["snapshot"]["id"],"candidate_id":fixture.work["id"],
        "candidate_revision":fixture.work["revision"],"disposition_id":disposition["id"]}),
    )
    .await;
    assert_eq!(
        opened["created"]["selected_option_id"],
        ranked["ranked_ids"][0]
    );
    let option = &prepared["options"][0];
    assert_eq!(
        opened["created"]["verification_plan_id"],
        option["verification_plan_id"]
    );
    let tenant: Uuid = sqlx::query_scalar("SELECT tenant_id FROM workspaces WHERE id=$1")
        .bind(workspace)
        .fetch_one(pool)
        .await
        .unwrap();
    let enrollment = admin::prepare_verifier_enrollment(pool, tenant, workspace)
        .await
        .unwrap()
        .try_commit()
        .await
        .unwrap();
    assert_ne!(enrollment.principal_id, fixture.owner_id);
    let config = fixture.root.join("native-pipeline-verifier.json");
    host_file(&config, &enrollment.auth);
    let mut verifier = Mcp::start(
        socket,
        &config,
        &Uuid::new_v4().to_string(),
        fixture.workspace_key,
    )
    .await;
    verifier.call("open_workspace", json!({})).await;
    let begun = super::run_binding::exercise(pool, workspace, owner, &opened, scope).await;
    super::phase_effect::exercise(
        owner,
        &mut verifier,
        super::phase_effect::PhaseEffectFixture {
            pool,
            workspace,
            opened: &opened,
            begun: &begun,
            root: fixture.root,
            socket,
        },
    )
    .await;
    verifier.finish().await;
}
