//! K1 continuation while the original source-bound Owner and distinct Verifier live.
use super::*;
use std::{path::Path, process::Command};

fn git(repo: &Path, args: &[&str]) -> String {
    let result = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .unwrap();
    assert!(result.status.success(), "source Git preflight failed");
    String::from_utf8(result.stdout).unwrap().trim().to_owned()
}

fn matches_frozen_source(
    selected: Uuid,
    repository: Uuid,
    frozen_worktrees: &[Uuid],
    frozen_digest: &str,
) -> bool {
    let mut identities = vec![(repository, selected)];
    identities.sort_by_key(|identity| identity.1);
    let calculated = format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&identities).unwrap())
    );
    frozen_worktrees == [selected] && calculated == frozen_digest
}

fn facts_current(now: i64, expires_at: i64) -> bool {
    now < expires_at
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn complete_and_verify_k1(
    pool: &PgPool,
    workspace: Uuid,
    owner: &mut Mcp,
    verifier: &mut Mcp,
    run: &Value,
    slice: &Value,
    open_effect: &Value,
    registered: &Value,
    source_path: &Path,
    source_head: &str,
    facts_expires_at: i64,
) {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    assert!(
        facts_current(now, facts_expires_at),
        "Owner facts expired before K1"
    );
    let run_id = id(&run["id"]);
    let worktree = id(&registered["id"]);
    let repository = id(&registered["repository_id"]);
    assert_eq!(registered["path"], json!(source_path));
    assert_eq!(git(source_path, &["rev-parse", "HEAD"]), source_head);
    assert!(git(source_path, &["status", "--porcelain"]).is_empty());
    let state = owner.call("get_state", json!({})).await;
    assert!(
        state["selected_worktrees"]
            .as_array()
            .unwrap()
            .iter()
            .any(|source| source["id"] == registered["id"])
    );
    let sources = route(owner, "query", "source.list", json!({"limit":25})).await;
    assert!(
        sources["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|source| source["id"] == registered["id"] && source["path"] == registered["path"])
    );

    let snapshot_id = id(&open_effect["material"]["source_snapshot_id"]);
    let snapshot: (Vec<Uuid>, String) = sqlx::query_as(
        "SELECT selected_worktree_ids,selected_sources_digest FROM scope_candidate_snapshots WHERE workspace_id=$1 AND id=$2",
    )
    .bind(workspace).bind(snapshot_id).fetch_one(pool).await.unwrap();
    assert!(
        matches_frozen_source(worktree, repository, &snapshot.0, &snapshot.1),
        "current selected source must be the exact frozen source identity"
    );
    let frozen: (Uuid, String, String) = sqlx::query_as(
        "SELECT source_snapshot_id,source_snapshot_digest,manifest_digest FROM pipeline_advice_contexts WHERE workspace_id=$1 AND manifest_digest=$2",
    )
    .bind(workspace).bind(open_effect["material"]["manifest_digest"].as_str().unwrap())
    .fetch_one(pool).await.unwrap();
    assert_eq!(frozen.0, snapshot_id);
    assert_eq!(frozen.1, open_effect["material"]["source_snapshot_digest"]);
    assert_eq!(frozen.2, open_effect["material"]["manifest_digest"]);

    let context = route(
        owner,
        "query",
        "slice.pipeline.context",
        json!({"run_id":run_id}),
    )
    .await;
    assert_eq!(context["run"]["id"], run["id"]);
    assert_eq!(context["run"]["revision"], run["revision"]);
    assert_eq!(context["run"]["status"], "active");
    assert_eq!(context["run"]["current_phase_id"], "K1");
    assert_eq!(
        context["run"]["selected_option_id"],
        slice["selected_option_id"]
    );
    assert_eq!(
        context["run"]["verification_plan_digest"],
        slice["verification_plan_digest"]
    );
    assert!(context["attempts"].as_array().unwrap().is_empty());
    let phase = &context["definition"]["phases"][0];
    assert_eq!(phase["id"], "K1");
    assert_eq!(
        phase["required_fields"],
        json!([
            "fit",
            "request",
            "parent",
            "preflight",
            "authority",
            "acceptance_checks",
            "route"
        ])
    );
    let slice_context = route(
        owner,
        "query",
        "slice.context",
        json!({"slice_id":slice["id"]}),
    )
    .await;
    let scope_context = route(
        owner,
        "query",
        "scope.context",
        json!({"scope_id":slice["scope_id"]}),
    )
    .await;
    assert_eq!(slice_context["id"], slice["id"]);
    assert_eq!(slice_context["revision"], slice["revision"]);
    assert_eq!(slice_context["scope_id"], scope_context["id"]);
    assert_eq!(slice_context["pipeline_run_id"], run["id"]);
    assert_eq!(scope_context["revision"], 1);
    assert_eq!(slice["state"], "open");
    assert_eq!(open_effect["material"]["slice"]["id"], slice["id"]);
    let request_id = Uuid::new_v4();
    let output = json!({
        "body":format!("K1 entry preflight for exact dev Git {source_head}, source worktree {worktree}, frozen snapshot {snapshot_id}; same selected run and current open parent verified via public reads. No source mutation or K2-K5 proof is claimed."),
        "producer_context_id":format!("active-jev-k1-owner:{run_id}"),
        "reference":format!("git:{source_head};source-snapshot:{snapshot_id}"),
        "fields":{
            "fit":"bounded_understood",
            "request":format!("Isolated Active JEV MVP selected Work at task-bound manifest {}", frozen.2),
            "parent": "current_confirmed",
            "preflight":"current_clear",
            "authority":"authorized",
            "acceptance_checks":"Exact source-bound optional advice; EM02-SCOPE@0.1 and EM02-PROTECT@0.1 preserved; K1-K5 selected plan and independent phase-effect proof. No production or installed-runtime claim.",
            "route":"none"
        },
        "verdict":"pass","dispositions":["satisfied"],
        "skill_reads":[],"resource_reads":[]
    });
    let completed = route(
        owner,
        "command",
        "slice.pipeline.phase.complete",
        json!({
            "request_id":request_id,"run_id":run_id,"run_revision":run["revision"],
            "phase_id":"K1","outcome":"completed","transition":"continue","output":output,
        }),
    )
    .await;
    let attempt_id: Uuid = sqlx::query_scalar(
        "SELECT id FROM slice_pipeline_phase_attempts WHERE workspace_id=$1 AND run_id=$2 AND request_id=$3",
    ).bind(workspace).bind(run_id).bind(request_id).fetch_one(pool).await.unwrap();
    assert_eq!(completed["context"]["run"]["current_phase_id"], "K2");
    let effect = route(
        verifier,
        "query",
        "pipeline.phase_effect.get",
        json!({"run_id":run_id,"attempt_id":attempt_id}),
    )
    .await;
    let material = &effect["material"];
    assert_eq!(material["run_id"], run["id"]);
    assert_eq!(material["phase_id"], "K1");
    assert_eq!(material["slice_id"], slice["id"]);
    assert_eq!(
        material["verification_plan_digest"],
        slice["verification_plan_digest"]
    );
    assert_eq!(material["selected_option_id"], slice["selected_option_id"]);
    assert_eq!(material["output"]["fields"]["preflight"], "current_clear");
    assert_ne!(
        effect["verifier_principal_id"],
        material["caller_principal_id"]
    );
    assert_ne!(effect["verifier_session_id"], material["caller_session_id"]);
    let attested = route(verifier, "command", "pipeline.phase_effect.verify", json!({
        "request_id":Uuid::new_v4(),"run_id":run_id,"attempt_id":attempt_id,
        "expected_effect_digest":effect["effect_digest"],"verdict":"pass",
        "observation":format!("Independent read of K1 output {} and frozen plan {} confirms current source identity and pass fields.", material["output_id"], frozen.2),
        "observed_output_digest":material["output_digest"],
        "summary":"Distinct Verifier matched the exact source-bound K1 receipt; K2-K5 remain unfinished."
    })).await;
    assert_eq!(attested["effect_digest"], effect["effect_digest"]);
    assert_eq!(attested["verdict"], "pass");
    println!(
        "s03_k1_phase_effect run={run_id} attempt={attempt_id} source_snapshot={snapshot_id} source_worktree={worktree} source_head={source_head} effect_digest={} verifier_verdict=pass next_phase=K2",
        effect["effect_digest"]
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn changed_worktree_identity_cannot_claim_frozen_source() {
        let old_worktree = Uuid::new_v4();
        let new_worktree = Uuid::new_v4();
        let repository = Uuid::new_v4();
        let digest = format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(&vec![(repository, old_worktree)]).unwrap())
        );
        assert!(matches_frozen_source(
            old_worktree,
            repository,
            &[old_worktree],
            &digest
        ));
        assert!(!matches_frozen_source(
            new_worktree,
            repository,
            &[old_worktree],
            &digest
        ));
    }

    #[test]
    fn expired_owner_facts_cannot_pass_k1() {
        assert!(facts_current(99, 100));
        assert!(!facts_current(100, 100));
        assert!(!facts_current(101, 100));
    }
}
