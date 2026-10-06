//! Backend binding regression for the existing DK2 K1 fixture; no model/body-reading claim.
use super::lifecycle_support::completion_request;
use super::{Mcp, PgPool, ResolvedPipeline, Value, json, route_error};
use uuid::Uuid;

pub(super) fn k1_request(context: &ResolvedPipeline) -> Value {
    assert_eq!(context.run()["definition_version"], "0.7.1-native.k1k5");
    assert_eq!(
        context.run()["definition_digest"],
        "93df97f4cb4458a18411b76005b29025a56234dc47650e4147ac5fdab3d30d89"
    );
    assert_eq!(context.current_phase().unwrap()["id"], "K1");
    let request = completion_request(context, "completed", "continue", None, false);
    for field in ["consumed_outputs", "consumed_inputs", "consumed_knowledge"] {
        assert!(request.get(field).is_none(), "backend owns {field}");
    }
    assert_eq!(request["output"]["skill_reads"], json!([]));
    assert_eq!(request["output"]["resource_reads"], json!([]));
    request
}

pub(super) async fn checkpoint(pool: &PgPool, context: &ResolvedPipeline) -> Value {
    let run = Uuid::parse_str(context.run()["id"].as_str().unwrap()).unwrap();
    sqlx::query_scalar("SELECT pg_catalog.jsonb_build_object('run',pg_catalog.to_jsonb(r),'workspace_run_ids',(SELECT COALESCE(pg_catalog.jsonb_agg(other.id ORDER BY other.id),'[]'::jsonb) FROM slice_pipeline_runs other WHERE other.tenant_id=r.tenant_id AND other.workspace_id=r.workspace_id),'attempts',(SELECT COALESCE(pg_catalog.jsonb_agg(pg_catalog.to_jsonb(a) ORDER BY a.id),'[]'::jsonb) FROM slice_pipeline_phase_attempts a WHERE a.tenant_id=r.tenant_id AND a.workspace_id=r.workspace_id AND a.run_id=r.id),'outputs',(SELECT COALESCE(pg_catalog.jsonb_agg(pg_catalog.to_jsonb(o) ORDER BY o.id),'[]'::jsonb) FROM slice_pipeline_phase_outputs o WHERE o.tenant_id=r.tenant_id AND o.workspace_id=r.workspace_id AND o.run_id=r.id),'bindings',(SELECT COALESCE(pg_catalog.jsonb_agg(pg_catalog.to_jsonb(b) ORDER BY b.phase_id),'[]'::jsonb) FROM slice_pipeline_output_bindings b WHERE b.tenant_id=r.tenant_id AND b.workspace_id=r.workspace_id AND b.run_id=r.id),'knowledge',(SELECT COALESCE(pg_catalog.jsonb_agg(pg_catalog.to_jsonb(m) ORDER BY m.id),'[]'::jsonb) FROM pipeline_knowledge_manifests m WHERE m.tenant_id=r.tenant_id AND m.workspace_id=r.workspace_id AND m.run_id=r.id)) FROM slice_pipeline_runs r WHERE r.id=$1")
        .bind(run).fetch_one(pool).await.unwrap()
}

pub(super) async fn reject_agent_ack(client: &mut Mcp, pool: &PgPool, context: &ResolvedPipeline) {
    let before = checkpoint(pool, context).await;
    let manifest = &context.details_data()["knowledge_resources"];
    let mut request = k1_request(context);
    request["consumed_knowledge"] =
        json!({"manifest_id":manifest["id"],"digest":manifest["digest"]});
    let denied = route_error(client, "command", "slice.pipeline.phase.complete", request).await;
    assert_eq!(denied["error"]["code"], "BACKEND_DERIVED_PROOF_REQUIRED");
    for (field, expected) in [
        ("code", "BACKEND_DERIVED_PROOF_REQUIRED"),
        ("rule", "WP3-PROOF-01"),
        ("path", "arguments.params.consumed_knowledge"),
        ("expected", "omitted; backend derives the proof"),
        ("actual", "agent-supplied value"),
        ("next_action", "omit_agent_supplied_proof"),
        ("required", "backend_derived_proof"),
    ] {
        assert_eq!(denied["error"]["refusal"][field], expected);
    }
    assert_eq!(checkpoint(pool, context).await, before);
}

pub(super) async fn persisted_binding(pool: &PgPool, context: &ResolvedPipeline, request: &Value) {
    let resources = &context.details_data()["knowledge_resources"];
    assert!(!resources["selected"].as_array().unwrap().is_empty());
    assert!(
        context.details_data()["knowledge"]["selected"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let request_id = Uuid::parse_str(request["request_id"].as_str().unwrap()).unwrap();
    let row: (Uuid, String, i64, Value, Value) = sqlx::query_as("SELECT knowledge_manifest_id,knowledge_manifest_digest,knowledge_workspace_generation,evidence_refs,request_payload FROM slice_pipeline_phase_attempts WHERE request_id=$1 AND run_id=$2")
        .bind(request_id).bind(Uuid::parse_str(context.run()["id"].as_str().unwrap()).unwrap())
        .fetch_one(pool).await.unwrap();
    assert_eq!(json!(row.0), resources["id"]);
    assert_eq!(json!(row.1), resources["digest"]);
    assert_eq!(json!(row.2), resources["workspace_generation"]);
    let refs = row
        .3
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r["kind"] == "knowledge_manifest")
        .collect::<Vec<_>>();
    assert_eq!(refs.len(), 1);
    assert_eq!(
        *refs[0],
        json!({"kind":"knowledge_manifest","reference":resources["id"],"revision":resources["workspace_generation"],"digest":resources["digest"]})
    );
    let decoded_request =
        serde_json::from_value::<tect_domain::CompletePipelinePhase>(request.clone())
            .expect("original public request must decode as CompletePipelinePhase");
    let expected_request = serde_json::to_value(decoded_request)
        .expect("original decoded agent request must serialize for persistence");
    assert_eq!(
        row.4, expected_request,
        "decoded agent request remains immutable"
    );
    assert!(row.4.get("consumed_knowledge").is_none());
}
