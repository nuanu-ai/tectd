use super::*;
use tect_application::{
    AuthoredScopeAlternative, AuthoredScopeSet, GuardedScopeAdviceRecord,
    ScopeAuthoredManifestRequest, ScopeAuthorityObserver, ScopeAuthorityOutcome,
    ScopeAuthorityRequest, ScopeManifestRecord, ScopeManifestSupplier, Sha256ScopeDigest, Store,
    TransactionMode,
};
use tect_domain::{
    ConfidenceBasisPoints, NormalizedScopeAdviceAnswer, NormalizedScopeAdviceAnswers,
    ScopeAdviceChoice, ScopeAdviceScoreBand, ScopeDecompositionKind, guard_scope_advice,
};
use tect_postgres::{PgScopeAuthoredManifestSupplier, PgScopeAuthorityObserver};

// Explicit fixture authorities stay separate rather than introducing a harness.
#[allow(clippy::too_many_arguments)]
pub(super) async fn selected_rankable(
    pool: &PgPool,
    store: &PgStore,
    auth: &tect_domain::HostAuth,
    tenant: Uuid,
    actor: Uuid,
    workspace: Uuid,
    native: &str,
    client: &mut Mcp,
    repo: &std::path::Path,
) -> (Uuid, i64) {
    // Existing public source planning helper produces a reviewed real candidate.
    identity(pool).await;
    let (context, candidate, goal) = source::ready(pool, client, repo).await;
    let candidate_set = id(&context["candidate_set"]["id"]);
    let mut draft = json!({"boundary":"ongoing","goals":[{
        "identity":{"id":goal["id"],"revision":goal["revision"]},"text":goal["text"],
        "source_ref_id":goal["source_ref_id"],"resolution":{"kind":"candidate","reference":{"id":candidate["id"]}}
    }],"evidence":[],"candidates":[{
        "identity":{"id":candidate["id"],"revision":candidate["revision"]},"title":candidate["title"],
        "outcome":candidate["outcome"],"trigger":candidate["trigger"],"delivered_behavior":candidate["delivered_behavior"],
        "proof":candidate["proof"],"includes":candidate["includes"],"excludes":candidate["excludes"],
        "dependencies":[],"coverage_goals":[{"id":goal["id"]}],"evidence":[]
    }],"blockers":[],"protected_changes":[]});
    for suffix in ["a", "b"] {
        draft["candidates"].as_array_mut().unwrap().push(json!({"identity":{"local":format!("exploratory-{suffix}")},
            "grounding":{"kind":"exploratory_unrequested","provenance":"source_authored_v2"},
            "title":format!("Unrequested dashboard {suffix}"),"outcome":"Optional dashboard","trigger":"Exploration",
            "delivered_behavior":"Show a dashboard","proof":"Optional visual check","coverage_goals":[]}));
    }
    let revision = context["candidate_set"]["revision"].as_i64().unwrap();
    let mut refs = context["snapshot"]["source_refs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| id(&v["id"]))
        .collect::<Vec<_>>();
    refs.sort();
    let session: Uuid = sqlx::query_scalar(
        "SELECT id FROM agent_sessions WHERE tenant_id=$1 AND native_session_id=$2",
    )
    .bind(tenant)
    .bind(native)
    .fetch_one(pool)
    .await
    .unwrap();
    let authority = Arc::new(PgScopeAuthorityObserver::new(
        store.clone(),
        Arc::new(tect_host::StaticCandidateGuidance),
    ));
    let observed = authority
        .observe(&ScopeAuthorityRequest {
            tenant_id: tenant,
            workspace_id: workspace,
            actor_id: actor,
            session_id: session,
            candidate_set_id: candidate_set,
        })
        .await
        .unwrap();
    let ScopeAuthorityOutcome::Authorized(observed) = observed else {
        panic!("fresh source authority")
    };
    let supplier = PgScopeAuthoredManifestSupplier::new(store.clone(), authority);
    identity(pool).await;
    let manifest = supplier
        .supply_authored(&ScopeAuthoredManifestRequest {
            tenant_id: tenant,
            observation: *observed,
            authored_scope_set: AuthoredScopeSet {
                expected_candidate_set_revision: revision,
                baseline_key: "baseline".into(),
                alternatives: vec![AuthoredScopeAlternative {
                    key: "baseline".into(),
                    kind: ScopeDecompositionKind::Cohesive,
                    draft: serde_json::from_value(draft.clone()).unwrap(),
                    covered_source_ref_ids: refs,
                }],
            },
        })
        .await
        .unwrap();
    let opportunity = Uuid::new_v4();
    let dispatch = Uuid::new_v4();
    let digest = &manifest.whole_set_digest;
    identity(pool).await;
    sqlx::query("INSERT INTO advisory_opportunity(id,tenant_id,workspace_id,work_item_kind,work_item_id,source_revision,session_id,authorized_actor_id,capability,decision_point,config_revision,session_preference,request_preference,policy_version,request_key,material_digest,state,primary_reason) VALUES($1,$2,$3,'scope_candidate_set',$4,$5,$6,$7,'scope_decomposition','scope.decomposition.before_selection',1,'use_workspace','use_workspace','fixture',$8,$9,'prepared','dispatch_authorized')")
        .bind(opportunity).bind(tenant).bind(workspace).bind(candidate_set).bind(revision.to_string()).bind(session).bind(actor)
        .bind(opportunity.to_string()).bind(digest).execute(pool).await.unwrap();
    identity(pool).await;
    let mut unit = store.begin(TransactionMode::ReadWrite).await.unwrap();
    unit.authenticate(auth).await.unwrap();
    unit.set_tenant(tenant).await.unwrap();
    unit.prepare_authored_scope_advisory_manifest(
        workspace,
        &ScopeManifestRecord {
            opportunity_id: opportunity,
            candidate_set_id: candidate_set,
            config_revision: 1,
            opportunity_material_digest: digest.clone(),
            manifest: manifest.clone(),
        },
        &"a".repeat(64),
    )
    .await
    .unwrap();
    unit.commit().await.unwrap();
    identity(pool).await;
    sqlx::query("INSERT INTO advisory_dispatch(id,tenant_id,workspace_id,opportunity_id,attempt_number,provider,model,configuration_snapshot,configuration_digest,material_digest,payload_digest,request_payload,response_payload,state,send_certainty,outcome,retry_basis,send_started_at,sealed_at) VALUES($1,$2,$3,$4,1,'fixture','fixture','{}',$5,$5,$5,'fixture','fixture','sealed','sent','provider_response','initial',clock_timestamp(),clock_timestamp())")
        .bind(dispatch).bind(tenant).bind(workspace).bind(opportunity).bind(digest).execute(pool).await.unwrap();
    identity(pool).await;
    sqlx::query("UPDATE advisory_opportunity SET state='advised',primary_reason='provider_response' WHERE id=$1")
        .bind(opportunity).execute(pool).await.unwrap();
    let request =
        tect_domain::ScopeAdviceRequest::from_manifest(&Sha256ScopeDigest, &manifest).unwrap();
    let advice = guard_scope_advice(
        &Sha256ScopeDigest,
        opportunity,
        &manifest,
        &request,
        &NormalizedScopeAdviceAnswers {
            answers: vec![NormalizedScopeAdviceAnswer {
                alternative_id: manifest.baseline_id.clone(),
                choice: ScopeAdviceChoice::Preferred,
                score: ScopeAdviceScoreBand::StrongFit,
                choice_confidence: ConfidenceBasisPoints(9000),
                score_confidence: ConfidenceBasisPoints(8000),
            }],
        },
    )
    .unwrap();
    identity(pool).await;
    let mut unit = store.begin(TransactionMode::ReadWrite).await.unwrap();
    unit.authenticate(auth).await.unwrap();
    unit.set_tenant(tenant).await.unwrap();
    unit.persist_guarded_scope_advice(
        workspace,
        &GuardedScopeAdviceRecord {
            opportunity_id: opportunity,
            candidate_set_id: candidate_set,
            dispatch_id: dispatch,
            dispatch_material_digest: digest.clone(),
            config_revision: 1,
            advice: advice.clone(),
        },
    )
    .await
    .unwrap();
    unit.commit().await.unwrap();
    let disposition=call(pool,client,"command","scope.advisory.disposition",json!({"opportunity_id":opportunity,
        "candidate_set_id":candidate_set,"request_id":Uuid::new_v4(),"advice_id":advice.id,"expected_revision":0,
        "action":"accept","selected_id":manifest.baseline_id,"items":[{"alternative_id":manifest.baseline_id,"state":"selected"}],
        "rationale":"Fixture source-authored selection"})).await;
    let saved=call(pool,client,"command","scope.candidates.save",json!({"kind":"draft","candidate_set_id":candidate_set,
        "revision":revision,"snapshot_id":context["snapshot"]["id"],"input_cursor":context["candidate_set"]["input_cursor"],
        "request_id":Uuid::new_v4(),"draft":draft,"selected_advisory":{"opportunity_id":opportunity,
            "disposition_id":disposition["id"],"selected_id":manifest.baseline_id,"alternative_key":"baseline"}})).await;
    (
        candidate_set,
        saved["context"]["candidate_set"]["revision"]
            .as_i64()
            .unwrap(),
    )
}
