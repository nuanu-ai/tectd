use super::*;
pub(super) async fn counts(pool: &PgPool, workspace: Uuid, set: Uuid) -> (i64, i64) {
    sqlx::query_as("SELECT revision,(SELECT count(*) FROM scope_candidate_drafts WHERE workspace_id=$1 AND candidate_set_id=$2) FROM scope_candidate_sets WHERE workspace_id=$1 AND id=$2")
        .bind(workspace).bind(set).fetch_one(pool).await.unwrap()
}
pub(super) async fn authored(pool: &PgPool, client: &mut Mcp, repo: &std::path::Path) -> Value {
    decomposition_parent::guard(pool).await;
    let (context, prior) = support::ready_source_candidate(client, repo).await;
    let set = &context["candidate_set"]["id"];
    let inputs = route(
        client,
        "query",
        "scope.candidates.context",
        json!({"candidate_set_id":set,"view":"inputs","limit":25}),
    )
    .await;
    let program = route(
        client,
        "query",
        "scope.candidates.context",
        json!({"candidate_set_id":set,"view":"program","limit":25}),
    )
    .await;
    let mut refs: Vec<Uuid> = inputs["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| Uuid::parse_str(item["input"]["source_ref_id"].as_str().unwrap()).unwrap())
        .collect();
    let source_ref = refs[0];
    refs.extend(
        program["program"]["field_refs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|field| Uuid::parse_str(field["id"].as_str().unwrap()).unwrap()),
    );
    refs.sort_unstable();
    refs.dedup();
    let draft = |title: &str| {
        json!({"boundary":"ongoing","goals":[{"identity":{"local":"goal"},
        "text":"Preserve source evidence","source_ref_id":source_ref,
        "resolution":{"kind":"candidate","reference":{"local":"candidate"}}}],
        "evidence":[],"candidates":[{"identity":{"local":"candidate"},"title":title,
        "outcome":"The preview cause is demonstrated","trigger":"Preview differs from settings",
        "delivered_behavior":"A bounded correction is selected","proof":"Direct source evidence is retained",
        "includes":["diagnosis"],"excludes":["deployment"],"dependencies":[],
        "coverage_goals":[{"local":"goal"}],"evidence":[]}],"blockers":[],"protected_changes":[],
        "supersessions":[{"candidate_id":prior["id"],"revision":prior["revision"],
        "reason":"Compare this authored option with the prior candidate","replacements":[{"local":"candidate"}]}]})
    };
    json!({"request_id":Uuid::new_v4(),"candidate_set_id":set,"authored_scope_set":{
        "expected_candidate_set_revision":context["candidate_set"]["revision"],"baseline_key":"baseline",
        "alternatives":[{"key":"baseline","kind":"cohesive","draft":draft("Cohesive diagnosis"),"covered_source_ref_ids":refs},
        {"key":"alternative","kind":"cohesive","draft":draft("Alternative diagnosis"),"covered_source_ref_ids":refs}]}})
}
