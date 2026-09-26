use super::*;
async fn tool(pool: &PgPool, client: &mut Mcp, name: &str, params: Value) -> Value {
    identity(pool).await;
    client.call(name, params).await
}
fn knowledge(params: &mut Value, context: &Value) {
    let manifest = &context["planning_knowledge"]["manifest"];
    if manifest["id"].is_string() {
        params["consumed_knowledge"] = json!({"manifest_id":manifest["id"],"digest":manifest["digest"],"workspace_generation":manifest["workspace_generation"]});
    }
}
// Same public source-planning sequence as native_planning/support, with the
// owned-cluster identity refreshed immediately before each mutating tool.
pub(super) async fn ready(
    pool: &PgPool,
    client: &mut Mcp,
    source: &std::path::Path,
) -> (Value, Value, Value) {
    tool(pool, client, "open_workspace", json!({})).await;
    let registered = tool(pool, client, "register_source", json!({"path":source})).await;
    tool(
        pool,
        client,
        "select_worktrees",
        json!({"worktree_ids":[registered["id"]]}),
    )
    .await;
    let begun=tool(pool,client,"begin_program",json!({"request_id":Uuid::new_v4(),"input":"Diagnose the incorrect preview, then select the smallest correction."})).await;
    let mut params = json!({"program_id":begun["program"]["id"],"revision":1,"input_cursor":1,"name":"Notification preview","intent":"Correct preview behavior from demonstrated evidence","basis":"The preview differs from saved settings","boundaries":"Preview diagnosis and bounded correction","constraints":"No deployment or adjacent notification work","success":"The cause and correction are verified","complete":true});
    knowledge(&mut params, &begun["program"]);
    let program = tool(pool, client, "save_program", params).await;
    let candidates=tool(pool,client,"begin_candidate_set",json!({"request_id":Uuid::new_v4(),"program_id":program["program"]["id"],"program_revision":program["program"]["revision"],"boundary":"ongoing","input":"Open one native Scope for diagnosis and its result-driven correction decision."})).await;
    let context = &candidates["context"];
    let inputs = tool(
        pool,
        client,
        "candidate_context",
        json!({"candidate_set_id":context["candidate_set"]["id"],"view":"inputs","limit":25}),
    )
    .await;
    let reference = &inputs["items"][0]["input"]["source_ref_id"];
    let mut params = json!({"kind":"draft","candidate_set_id":context["candidate_set"]["id"],"revision":1,"snapshot_id":context["snapshot"]["id"],"input_cursor":1,"request_id":Uuid::new_v4(),
        "draft":{"boundary":"ongoing","goals":[{"identity":{"local":"goal"},"text":"Explain preview deviation and bound correction","source_ref_id":reference,"resolution":{"kind":"candidate","reference":{"local":"scope"}}}],"evidence":[],"candidates":[{"identity":{"local":"scope"},"title":"Preview diagnosis and correction decision","outcome":"The cause is demonstrated and the correction path selected","trigger":"Preview differs","delivered_behavior":"Cause and bounded follow-up are available","proof":"Direct evidence is retained","includes":["diagnosis","decision"],"excludes":["deployment"],"dependencies":[],"coverage_goals":[{"local":"goal"}],"evidence":[]}],"blockers":[],"protected_changes":[]}});
    knowledge(&mut params, context);
    let saved = tool(pool, client, "save_candidate_set", params).await;
    let candidate = saved["draft"]["candidates"][0].clone();
    let mut params = json!({"kind":"review","candidate_set_id":saved["context"]["candidate_set"]["id"],"revision":saved["context"]["candidate_set"]["revision"],"snapshot_id":context["snapshot"]["id"],"input_cursor":saved["context"]["candidate_set"]["input_cursor"],"request_id":Uuid::new_v4(),"review":{"verdict":"ready","summary":"Bounded, vertical and ready","findings":[],"candidate_decisions":[{"candidate_id":candidate["id"],"decision":"accept","rationale":"Coherent Scope"}]}});
    knowledge(&mut params, &saved["context"]);
    let reviewed = tool(pool, client, "save_candidate_set", params).await;
    (
        reviewed["context"].clone(),
        candidate,
        saved["draft"]["goals"][0].clone(),
    )
}
