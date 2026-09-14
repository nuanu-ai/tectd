use serde_json::{Value, json};
use std::{fs, io::Write, os::unix::fs::OpenOptionsExt, path::Path, process::Command};

fn sha256(path: &Path) -> String {
    let output = Command::new("shasum")
        .args(["-a", "256"])
        .arg(path)
        .output()
        .unwrap();
    assert!(output.status.success());
    String::from_utf8(output.stdout)
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap()
        .to_owned()
}

fn git_head(repo: &Path) -> String {
    let output = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(repo)
        .output()
        .unwrap();
    assert!(output.status.success());
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

pub struct Evidence<'a> {
    pub path: &'a Path,
    pub repo: &'a Path,
    pub owner_count: &'a Value,
    pub foreign_count: &'a Value,
    pub foreign_nodes: &'a Value,
    pub foreign_edges: &'a Value,
    pub peer_same_principal: bool,
    pub revocation_error: &'a Value,
    pub role_constraint: String,
    pub manifest_snapshot: Value,
}

pub fn write(value: Evidence<'_>) {
    let source = |path: &str| sha256(&value.repo.join(path));
    let proof = json!({
        "test":"native_graph_projection_ancestry_access_and_bounds_are_exact",
        "git_head":git_head(value.repo),
        "model_configured":false,
        "paths":["targets","depends_on","uses_asset","in_environment","derived_from","bound_to"],
        "slice_phase_qualifier":true,
        "populated_active_manifest_unchanged_by_lexical_search":true,
        "populated_active_manifest_unchanged_by_graph_search":true,
        "populated_manifest_snapshot":value.manifest_snapshot,
        "incoming_traversed_in_reverse":true,
        "cycle_terminated":true,
        "ancestry_filters":["program","scope","slice"],
        "owner_visible_corpus_count":value.owner_count,
        "foreign_visible_corpus_count":value.foreign_count,
        "foreign_graph_nodes_visited":value.foreign_nodes,
        "foreign_graph_edges_visited":value.foreign_edges,
        "supported_peer_same_principal":value.peer_same_principal,
        "membership_revocation_error":value.revocation_error,
        "principal_role_constraint":value.role_constraint,
        "non_owner_workspace_member_executable":false,
        "binary_sha256":{
            "tectd":sha256(Path::new(env!("CARGO_BIN_EXE_tectd"))),
            "tectd_mcp":sha256(Path::new(env!("CARGO_BIN_EXE_tectd-mcp"))),
            "tect_admin":sha256(Path::new(env!("CARGO_BIN_EXE_tect-admin"))),
            "test":sha256(&std::env::current_exe().unwrap())
        },
        "source_sha256":{
            "test":source("crates/cli/tests/knowledge_search_graph.rs"),
            "fixture":source("crates/cli/tests/knowledge_search_graph/fixture.rs"),
            "proof_writer":source("crates/cli/tests/knowledge_search_graph/proof.rs"),
            "host_schema":source("crates/host/src/api/knowledge_search_schema.rs"),
            "host_route":source("crates/host/src/knowledge_search_tools.rs"),
            "host_output":source("crates/host/src/knowledge_search_output.rs"),
            "postgres_query":source("crates/postgres/src/knowledge_search/query.rs"),
            "postgres_graph":source("crates/postgres/src/knowledge_search/graph.rs"),
            "postgres_resource":source("crates/postgres/src/knowledge_search/resource.rs")
        }
    });
    let mut file = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .open(value.path)
        .unwrap();
    file.write_all(&serde_json::to_vec_pretty(&proof).unwrap())
        .unwrap();
}
