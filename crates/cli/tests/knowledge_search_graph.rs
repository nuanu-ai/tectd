#[path = "knowledge_search_graph/fixture.rs"]
mod graph_fixture;
#[path = "knowledge_search_graph/proof.rs"]
mod graph_proof;
#[path = "pipeline_execution/knowledge_lifecycle_support.rs"]
#[allow(dead_code)]
mod knowledge_lifecycle_support;
#[allow(dead_code)]
mod recovery_support;
#[path = "native_planning/support.rs"]
#[allow(dead_code)]
mod support;
#[path = "knowledge_search_native/target.rs"]
mod target;

use graph_fixture::{binding, document, graph_params};
use knowledge_lifecycle_support::commit_create;
use recovery_support::{Daemon, Mcp, host_file, private_temp, tagged_url};
use serde_json::{Value, json};
use sqlx::PgPool;
use std::{collections::BTreeSet, path::PathBuf};
use support::{repository, route, route_error};
use tect_postgres::admin;
use uuid::Uuid;

async fn search(client: &mut Mcp, params: Value) -> Value {
    route(client, "query", "knowledge.search", params).await
}

fn result_ids(response: &Value) -> BTreeSet<Uuid> {
    response["results"]
        .as_array()
        .unwrap()
        .iter()
        .map(|result| Uuid::parse_str(result["unit_id"].as_str().unwrap()).unwrap())
        .collect()
}

fn hop_for(response: &Value, unit: Uuid) -> &Value {
    &response["results"]
        .as_array()
        .unwrap()
        .iter()
        .find(|result| result["unit_id"] == unit.to_string())
        .unwrap()["reasons"]
        .as_array()
        .unwrap()
        .iter()
        .find(|reason| reason["kind"] == "graph_path")
        .unwrap()["path"]["hops"][0]
}

async fn assert_edge(
    client: &mut Mcp,
    unit: Uuid,
    seed: &str,
    relation: &str,
    path: &[&str],
    binding: Option<Value>,
) -> Value {
    let response = search(client, graph_params(seed, relation, binding)).await;
    assert!(result_ids(&response).contains(&unit), "{response}");
    let hop = hop_for(&response, unit);
    assert_eq!(hop["relation"], relation);
    assert_eq!(hop["traversed_in_reverse"], true);
    assert_eq!(
        hop["predicate_path"],
        Value::Array(path.iter().map(|value| json!(value)).collect())
    );
    assert_eq!(response["vector_status"], "not_requested");
    response
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn native_graph_projection_ancestry_access_and_bounds_are_exact() {
    if std::env::var("TECT_TEST_DK3_GRAPH").as_deref() != Ok("1") {
        return;
    }
    let admin_url = std::env::var("TECT_TEST_ADMIN_URL").unwrap();
    let runtime_url = std::env::var("TECT_TEST_RUNTIME_URL").unwrap();
    let role = std::env::var("TECT_TEST_RUNTIME_ROLE").unwrap();
    let evidence = PathBuf::from(std::env::var_os("TECT_TEST_DK3_GRAPH_EVIDENCE").unwrap());
    let pool = PgPool::connect(&admin_url).await.unwrap();
    admin::migrate(&pool, &role).await.unwrap();
    tect_postgres::enable_durable_knowledge(&pool, &role)
        .await
        .unwrap();

    let temp = private_temp();
    let root = temp.path().canonicalize().unwrap();
    let repo = root.join("source");
    repository(&repo);
    let socket = root.join("graph.sock");
    let runtime = tagged_url(&runtime_url, &format!("dk3-graph-{}", Uuid::new_v4()));
    let _daemon = Daemon::start(&runtime, socket.clone()).await;
    let enrollment = admin::enroll_host(&pool, None, vec![root.to_string_lossy().into_owned()])
        .await
        .unwrap();
    let config = root.join("owner.json");
    host_file(&config, &enrollment.auth);
    let workspace_key = format!("dk3-graph-{}", Uuid::new_v4());
    let mut owner = Mcp::start(
        &socket,
        &config,
        &Uuid::new_v4().to_string(),
        &workspace_key,
    )
    .await;
    let target_a = target::open(&mut owner, &repo, &pool, "graph-a").await;
    let target_b = target::open(&mut owner, &repo, &pool, "graph-b").await;
    let workspace: Uuid =
        sqlx::query_scalar("SELECT id FROM workspaces WHERE tenant_id=$1 AND key=$2")
            .bind(enrollment.tenant_id)
            .bind(&workspace_key)
            .fetch_one(&pool)
            .await
            .unwrap();
    let program_a = Uuid::parse_str(target_a["program_id"].as_str().unwrap()).unwrap();
    let scope_a = Uuid::parse_str(target_a["scope_id"].as_str().unwrap()).unwrap();
    let slice_a = Uuid::parse_str(target_a["slice_id"].as_str().unwrap()).unwrap();
    let program_b = Uuid::parse_str(target_b["program_id"].as_str().unwrap()).unwrap();
    let scope_b = Uuid::parse_str(target_b["scope_id"].as_str().unwrap()).unwrap();
    let slice_revision: i64 = sqlx::query_scalar(
        "SELECT revision FROM native_slices WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3",
    )
    .bind(enrollment.tenant_id)
    .bind(workspace)
    .bind(slice_a)
    .fetch_one(&pool)
    .await
    .unwrap();
    let pipeline = route(
        &mut owner,
        "command",
        "slice.pipeline.begin",
        json!({"request_id":Uuid::new_v4(),"scope_id":scope_a,"slice_id":slice_a,
            "slice_revision":slice_revision,"delivery_mode":"phasewise",
            "qualification_reason":"Create an exact native SlicePhase graph binding."}),
    )
    .await;
    let phase_id = pipeline["created"]["run"]["current_phase_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let shared = "urn:tect:dk3:graph:shared-target-source";
    let dependency = "urn:tect:dk3:graph:dependency";
    let environment = "urn:tect:dk3:graph:environment";
    let asset = "urn:tect:dk3:graph:asset";
    let bindings = json!([
        binding(json!({"kind":"program","program_id":program_a})),
        binding(json!({"kind":"scope","scope_id":scope_a})),
        binding(json!({"kind":"slice","scope_id":scope_a,"slice_id":slice_a})),
        binding(
            json!({"kind":"slice_phase","scope_id":scope_a,"slice_id":slice_a,
            "phase_id":phase_id.clone()})
        )
    ]);
    let rich = commit_create(
        &mut owner,
        document(
            "Graph rich public",
            "workspace_members",
            bindings,
            shared,
            dependency,
            environment,
            Some(asset),
        ),
    )
    .await;
    let rich_id = Uuid::parse_str(
        rich.receipt["applied_operations"][0]["unit_id"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    let rich_iri = rich.exact["document"]["unit_iri"]
        .as_str()
        .unwrap()
        .to_owned();
    let run_id = Uuid::parse_str(pipeline["created"]["run"]["id"].as_str().unwrap()).unwrap();
    let manifest_snapshot =
        graph_fixture::assert_search_preserves_manifest(&mut owner, &pool, run_id, rich_id, shared)
            .await;
    let sibling = commit_create(
        &mut owner,
        document(
            "Graph sibling public",
            "workspace_members",
            json!([binding(json!({"kind":"program","program_id":program_a}))]),
            shared,
            "urn:tect:dk3:graph:sibling-dependency",
            environment,
            None,
        ),
    )
    .await;
    let sibling_id = Uuid::parse_str(
        sibling.receipt["applied_operations"][0]["unit_id"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    let private_marker = "Private graph marker must not cross tenants";
    let private = commit_create(
        &mut owner,
        document(
            private_marker,
            "owners_only",
            json!([binding(json!({"kind":"workspace"}))]),
            "urn:tect:dk3:graph:private",
            "urn:tect:dk3:graph:private-dependency",
            environment,
            None,
        ),
    )
    .await;
    let private_id = Uuid::parse_str(
        private.receipt["applied_operations"][0]["unit_id"]
            .as_str()
            .unwrap(),
    )
    .unwrap();

    let v2 = "urn:tect:dk:v2:";
    assert_edge(
        &mut owner,
        rich_id,
        shared,
        "targets",
        &[
            &format!("{v2}targets"),
            &format!("{v2}entry"),
            &format!("{v2}iriValue"),
        ],
        None,
    )
    .await;
    assert_edge(
        &mut owner,
        rich_id,
        asset,
        "uses_asset",
        &[
            &format!("{v2}devopsSection"),
            &format!("{v2}assets"),
            &format!("{v2}entry"),
            &format!("{v2}iriValue"),
        ],
        None,
    )
    .await;
    assert_edge(
        &mut owner,
        rich_id,
        dependency,
        "depends_on",
        &[
            &format!("{v2}runbookSection"),
            &format!("{v2}dependencies"),
            &format!("{v2}entry"),
            &format!("{v2}iriValue"),
        ],
        None,
    )
    .await;
    assert_edge(
        &mut owner,
        rich_id,
        environment,
        "in_environment",
        &[
            &format!("{v2}runbookSection"),
            &format!("{v2}targetEnvironments"),
            &format!("{v2}entry"),
            &format!("{v2}iriValue"),
        ],
        None,
    )
    .await;
    assert_edge(
        &mut owner,
        rich_id,
        shared,
        "derived_from",
        &[
            &format!("{v2}sources"),
            &format!("{v2}entry"),
            &format!("{v2}uri"),
        ],
        None,
    )
    .await;
    let bound_iri = format!(
        "urn:tect:workspace:{}:{workspace}:program:{program_a}",
        enrollment.tenant_id
    );
    let bound = assert_edge(
        &mut owner,
        rich_id,
        &bound_iri,
        "bound_to",
        &[
            &format!("{v2}bindings"),
            &format!("{v2}entry"),
            &format!("{v2}target"),
        ],
        None,
    )
    .await;
    let bound_hop = hop_for(&bound, rich_id);
    assert_eq!(bound_hop["binding"]["purpose"], "reference");
    assert_eq!(
        bound_hop["binding"]["version_resolution"]["kind"],
        "current_accepted"
    );
    assert!(bound_hop["binding"].get("phase_id").is_none());
    let phase_iri = format!(
        "urn:tect:workspace:{}:{workspace}:scope:{scope_a}:slice:{slice_a}:phase",
        enrollment.tenant_id
    );
    let phase_bound = assert_edge(
        &mut owner,
        rich_id,
        &phase_iri,
        "bound_to",
        &[
            &format!("{v2}bindings"),
            &format!("{v2}entry"),
            &format!("{v2}target"),
        ],
        None,
    )
    .await;
    let phase_hop = hop_for(&phase_bound, rich_id);
    assert_eq!(phase_hop["binding"]["purpose"], "reference");
    assert_eq!(
        phase_hop["binding"]["version_resolution"]["kind"],
        "current_accepted"
    );
    assert_eq!(phase_hop["binding"]["phase_id"], phase_id);

    for binding in [
        json!({"kind":"program","program_id":program_a}),
        json!({"kind":"scope","scope_id":scope_a}),
        json!({"kind":"slice","scope_id":scope_a,"slice_id":slice_a}),
    ] {
        let response = search(&mut owner, json!({"mode":"lexical","query":"Graph rich public",
            "binding":binding,"limit":16,"corpus_limit":64,"purpose":"Verify true target ancestry."})).await;
        assert!(result_ids(&response).contains(&rich_id));
    }
    for binding in [
        json!({"kind":"program","program_id":program_b}),
        json!({"kind":"scope","scope_id":scope_b}),
    ] {
        let response = search(&mut owner, json!({"mode":"lexical","query":"Graph rich public",
            "binding":binding,"limit":16,"corpus_limit":64,"purpose":"Reject unrelated target ancestry."})).await;
        assert!(!result_ids(&response).contains(&rich_id));
    }
    let missing = route_error(&mut owner, "query", "knowledge.search",
        json!({"mode":"lexical","query":"Graph rich public","binding":{"kind":"scope","scope_id":Uuid::new_v4()},
            "limit":16,"corpus_limit":64,"purpose":"Reject a missing native target."})).await;
    assert_eq!(missing["error"]["code"], "not_found");

    let limited = search(
        &mut owner,
        json!({"mode":"graph_search","seeds":[shared],
        "relations":["targets"],"direction":"incoming","max_depth":1,"limit":1,
        "corpus_limit":64,"purpose":"Prove bounded multi-result traversal."}),
    )
    .await;
    assert_eq!(limited["bounds"]["results_returned"], 1);
    assert_eq!(limited["results"].as_array().unwrap().len(), 1);
    assert_eq!(limited["bounds"]["results_truncated"], true);
    assert_eq!(limited["bounds"]["visible_corpus_count"], 3);
    assert!(limited["bounds"]["graph_nodes_visited"].as_u64().unwrap() <= 4_096);
    assert!(limited["bounds"]["graph_edges_visited"].as_u64().unwrap() <= 32_768);
    let cycle = search(
        &mut owner,
        json!({"mode":"graph_search","seeds":[rich_iri],
        "relations":["targets","derived_from"],"direction":"both","max_depth":4,
        "limit":16,"corpus_limit":64,"purpose":"Prove cycle-safe traversal."}),
    )
    .await;
    assert!(cycle["bounds"]["depth_reached"].as_u64().unwrap() < 4);
    assert!(cycle["bounds"]["graph_nodes_visited"].as_u64().unwrap() <= 4_096);
    assert!(cycle["bounds"]["graph_edges_visited"].as_u64().unwrap() <= 32_768);
    assert_eq!(cycle["bounds"]["graph_budget_exhausted"], false);

    let other = admin::enroll_host(&pool, None, vec![root.to_string_lossy().into_owned()])
        .await
        .unwrap();
    let other_config = root.join("other-tenant.json");
    host_file(&other_config, &other.auth);
    let mut foreign = Mcp::start(
        &socket,
        &other_config,
        &Uuid::new_v4().to_string(),
        &format!("dk3-foreign-{}", Uuid::new_v4()),
    )
    .await;
    foreign.call("open_workspace", json!({})).await;
    let absent = search(
        &mut foreign,
        graph_params("urn:tect:dk3:graph:private", "targets", None),
    )
    .await;
    assert_eq!(absent["results"], json!([]));
    assert_eq!(absent["bounds"]["visible_corpus_count"], 0);
    assert_eq!(absent["bounds"]["graph_nodes_visited"], 1);
    assert_eq!(absent["bounds"]["graph_edges_visited"], 0);
    assert!(!absent.to_string().contains(private_marker));
    assert!(!result_ids(&absent).contains(&private_id));
    let owner_private = search(
        &mut owner,
        graph_params("urn:tect:dk3:graph:private", "targets", None),
    )
    .await;
    assert!(result_ids(&owner_private).contains(&private_id));
    assert!(
        result_ids(&search(&mut owner, graph_params(shared, "targets", None)).await)
            .contains(&sibling_id)
    );

    let peer_enrollment = admin::enroll_host(
        &pool,
        Some(enrollment.tenant_id),
        vec![root.to_string_lossy().into_owned()],
    )
    .await
    .unwrap();
    assert_eq!(peer_enrollment.principal_id, enrollment.principal_id);
    let peer_config = root.join("peer-owner.json");
    host_file(&peer_config, &peer_enrollment.auth);
    let mut peer = Mcp::start(
        &socket,
        &peer_config,
        &Uuid::new_v4().to_string(),
        &workspace_key,
    )
    .await;
    peer.call("open_workspace", json!({})).await;
    let peer_private = search(
        &mut peer,
        graph_params("urn:tect:dk3:graph:private", "targets", None),
    )
    .await;
    assert!(result_ids(&peer_private).contains(&private_id));
    sqlx::query(
        "DELETE FROM memberships WHERE tenant_id=$1 AND workspace_id=$2 AND principal_id=$3",
    )
    .bind(enrollment.tenant_id)
    .bind(workspace)
    .bind(peer_enrollment.principal_id)
    .execute(&pool)
    .await
    .unwrap();
    let revoked = route_error(
        &mut peer,
        "query",
        "knowledge.search",
        graph_params("urn:tect:dk3:graph:private", "targets", None),
    )
    .await;
    assert_eq!(revoked["error"]["code"], "forbidden");
    assert!(!revoked.to_string().contains(private_marker));

    let principal_role_constraint: String = sqlx::query_scalar(
        "SELECT pg_catalog.pg_get_constraintdef(oid) FROM pg_catalog.pg_constraint \
         WHERE conname='principals_role_check' AND conrelid='principals'::regclass",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(principal_role_constraint.contains("role = 'owner'"));
    graph_proof::write(graph_proof::Evidence {
        path: &evidence,
        repo: std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap(),
        owner_count: &owner_private["bounds"]["visible_corpus_count"],
        foreign_count: &absent["bounds"]["visible_corpus_count"],
        foreign_nodes: &absent["bounds"]["graph_nodes_visited"],
        foreign_edges: &absent["bounds"]["graph_edges_visited"],
        peer_same_principal: peer_enrollment.principal_id == enrollment.principal_id,
        revocation_error: &revoked["error"]["code"],
        role_constraint: principal_role_constraint,
        manifest_snapshot,
    });

    foreign.finish().await;
    peer.finish().await;
    owner.finish().await;
}
