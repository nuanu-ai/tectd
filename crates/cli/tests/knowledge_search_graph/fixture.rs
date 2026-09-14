use crate::{
    recovery_support::{Mcp, action_params, find_action},
    support::route,
};
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;

pub fn binding(target: Value) -> Value {
    json!({"target":target,"purpose":"reference",
        "version_resolution":{"kind":"current_accepted"}})
}

pub fn graph_params(seed: &str, relation: &str, binding: Option<Value>) -> Value {
    let mut value = json!({"mode":"graph_search","seeds":[seed],"relations":[relation],
        "direction":"incoming","max_depth":1,"limit":16,"corpus_limit":64,
        "purpose":"Verify exact native graph projection and access bounds."});
    if let Some(binding) = binding {
        value["binding"] = binding;
    }
    value
}

pub fn document(
    label: &str,
    access: &str,
    bindings: Value,
    target_iri: &str,
    dependency_iri: &str,
    environment_iri: &str,
    asset_iri: Option<&str>,
) -> Value {
    let mut fixture: Value = serde_json::from_str(include_str!(
        "../../../postgres/src/knowledge_lifecycle/rdf/fixtures/runbook.json"
    ))
    .unwrap();
    let value = &mut fixture["document"];
    value["title"] = json!(label);
    value["canonical_text"] = json!(format!("Exact graph fixture {label}."));
    value["target_iris"] = json!([target_iri]);
    value["sources"][0]["snapshot"]["title"] = json!(format!("Source for {label}"));
    value["sources"][0]["snapshot"]["uri"] = json!(target_iri);
    value["sources"][0]["snapshot"]["text"] = json!(format!("Exact source for {label}."));
    value["bindings"] = bindings;
    value["access_scope"] = json!(access);
    value["sections"]["runbook"]["target_environment_iris"] = json!([environment_iri]);
    value["sections"]["runbook"]["dependency_iris"] = json!([dependency_iri]);
    if let Some(asset_iri) = asset_iri {
        let devops: Value = serde_json::from_str(include_str!(
            "../../../postgres/src/knowledge_lifecycle/rdf/fixtures/devops.json"
        ))
        .unwrap();
        value["profiles"]
            .as_array_mut()
            .unwrap()
            .push(json!("devops"));
        value["sections"]["devops"] = devops["document"]["sections"]["devops"].clone();
        value["sections"]["devops"]["asset_iris"] = json!([asset_iri]);
        value["sections"]["devops"]["environment_iris"] =
            json!([format!("{environment_iri}:devops")]);
    }
    value.clone()
}

pub async fn assert_search_preserves_manifest(
    client: &mut Mcp,
    pool: &PgPool,
    run_id: Uuid,
    unit_id: Uuid,
    seed: &str,
) -> Value {
    let stale = route(
        client,
        "query",
        "slice.pipeline.context",
        json!({"run_id":run_id}),
    )
    .await;
    assert_eq!(stale["knowledge_resource_status"]["state"], "stale");
    let refresh = find_action(&stale, "pipeline.knowledge_refresh").unwrap();
    route(
        client,
        "command",
        "pipeline.knowledge_refresh",
        action_params(refresh).clone(),
    )
    .await;
    let current = route(
        client,
        "query",
        "slice.pipeline.context",
        json!({"run_id":run_id}),
    )
    .await;
    assert_eq!(current["knowledge_resource_status"]["state"], "current");
    assert!(
        current["knowledge_resources"]["selected"]
            .as_array()
            .unwrap()
            .iter()
            .any(|resource| resource["unit_id"] == unit_id.to_string())
    );
    let before = manifest_snapshot(pool, run_id).await;
    assert!(before["manifest_count"].as_i64().unwrap() > 0);
    route(
        client,
        "query",
        "knowledge.search",
        json!({"mode":"lexical","query":"Graph rich public","limit":16,
            "corpus_limit":64,"purpose":"Prove populated phase context is read-only."}),
    )
    .await;
    route(
        client,
        "query",
        "knowledge.search",
        graph_params(seed, "targets", None),
    )
    .await;
    assert_eq!(manifest_snapshot(pool, run_id).await, before);
    before
}

pub async fn manifest_snapshot(pool: &PgPool, run_id: Uuid) -> Value {
    sqlx::query_scalar(
        r#"SELECT pg_catalog.jsonb_build_object(
           'run',pg_catalog.jsonb_build_object(
             'revision',r.revision,'current_phase_id',r.current_phase_id,
             'current_phase_ordinal',r.current_phase_ordinal,
             'knowledge_manifest_id',r.knowledge_manifest_id,
             'knowledge_manifest_digest',r.knowledge_manifest_digest),
           'workspace_generation',(SELECT s.generation FROM workspace_knowledge_state s
             WHERE s.tenant_id=r.tenant_id AND s.workspace_id=r.workspace_id),
           'active_binding_count',(SELECT count(*) FROM knowledge_bindings b
             WHERE b.tenant_id=r.tenant_id AND b.workspace_id=r.workspace_id AND b.active),
           'manifest_count',(SELECT count(*) FROM pipeline_knowledge_manifests m
             WHERE m.tenant_id=r.tenant_id AND m.workspace_id=r.workspace_id AND m.run_id=r.id),
           'manifests',(SELECT COALESCE(pg_catalog.jsonb_agg(to_jsonb(m) ORDER BY m.id),'[]'::jsonb)
             FROM pipeline_knowledge_manifests m WHERE m.tenant_id=r.tenant_id
             AND m.workspace_id=r.workspace_id AND m.run_id=r.id))
         FROM slice_pipeline_runs r WHERE r.id=$1"#,
    )
    .bind(run_id)
    .fetch_one(pool)
    .await
    .unwrap()
}
