macro_rules! verify {
    ($actor:ident, $advice:ident, $after:ident, $audit_query:ident, $auth:ident, $candidate:ident, $context:ident, $cursor:ident, $enrollment:ident, $first:ident, $foreign:ident, $foreign_unit:ident, $manifest:ident, $observation:ident, $opportunity:ident, $other_opportunity:ident, $page:ident, $pool:ident, $reason:ident, $request:ident, $result:ident, $selected_id:ident, $service:ident, $session:ident, $state:ident, $store:ident, $tenant:ident, $third_opportunity:ident, $unit:ident, $verifier_session:ident, $workspace:ident $(,)?) => {

    let item = ScopeDispositionItem {
        alternative_id: $manifest.baseline_id.clone(),
        $state: ScopeDispositionItemState::Selected,
    };
    let mut partial = ScopeDispositionRequest {
        request_id: Uuid::new_v4(),
        advice_id: $advice.id.clone(),
        expected_revision: 0,
        action: ScopeDispositionAction::Accept,
        $selected_id: Some($manifest.baseline_id.clone()),
        items: vec![],
        rationale: "accept".into(),
    };
    set_config(&$pool, $tenant, $workspace, false).await;
    let mut $unit = rw(&$store, &$enrollment.$auth, $tenant).await;
    assert_eq!(
        $unit.cas_scope_advisory_disposition(
            $workspace,
            ScopeDispositionRecord {
                opportunity_id: $opportunity,
                candidate_set_id: $candidate,
                actor_id: $actor,
                session_id: $session,
                $request: partial.clone(),
            }
        )
        .await,
        Err(Error::StaleContext)
    );
    drop($unit);
    set_config(&$pool, $tenant, $workspace, true).await;
    let mut $unit = rw(&$store, &$enrollment.$auth, $tenant).await;
    assert_eq!(
        $unit.cas_scope_advisory_disposition(
            $workspace,
            ScopeDispositionRecord {
                opportunity_id: $opportunity,
                candidate_set_id: $candidate,
                actor_id: $actor,
                session_id: $session,
                $request: partial.clone()
            }
        )
        .await,
        Err(Error::InvalidArguments)
    );
    partial.items = vec![item];
    drop($unit);
    let $context = tect_domain::RequestContext {
        $auth: $enrollment.$auth.clone(),
        native_session_id: $session.to_string(),
        workspace_key: format!("scope-live-{workspace}", $workspace = $workspace),
    };
    let disposition = $service
        .decide_scope_advisory(&$context, $opportunity, $candidate, partial.clone())
        .await
        .unwrap();
    assert_eq!(
        $service
            .decide_scope_advisory(&$context, $opportunity, $candidate, partial.clone())
            .await
            .unwrap(),
        disposition
    );
    let other_disposition_opportunity = if $first.is_ok() {
        $other_opportunity
    } else {
        $third_opportunity
    };
    let separate_dispositions: i64 = sqlx::query_scalar(
        "SELECT count(DISTINCT opportunity_id) FROM advisory_scope_disposition WHERE tenant_id=$1 AND workspace_id=$2 AND opportunity_id = ANY($3)",
    )
    .bind($tenant).bind($workspace).bind(vec![$opportunity, other_disposition_opportunity])
    .fetch_one(&$pool).await.unwrap();
    assert_eq!(separate_dispositions, 2);
    let mut changed_lineage = partial.clone();
    changed_lineage.advice_id = ScopeAdviceId(D.into());
    let mut $unit = rw(&$store, &$enrollment.$auth, $tenant).await;
    assert_eq!(
        $unit.cas_scope_advisory_disposition(
            $workspace,
            ScopeDispositionRecord {
                opportunity_id: $opportunity,
                candidate_set_id: $candidate,
                actor_id: $actor,
                session_id: $session,
                $request: changed_lineage
            }
        )
        .await,
        Err(Error::InputConflict)
    );
    drop($unit);
    let mut competing = partial.clone();
    competing.request_id = Uuid::new_v4();
    let mut $unit = rw(&$store, &$enrollment.$auth, $tenant).await;
    assert_eq!(
        $unit.cas_scope_advisory_disposition(
            $workspace,
            ScopeDispositionRecord {
                opportunity_id: $opportunity,
                candidate_set_id: $candidate,
                actor_id: $actor,
                session_id: $session,
                $request: competing
            }
        )
        .await,
        Err(Error::StaleRevision)
    );
    drop($unit);

    let $observation = FreshScopeObservation {
        source: $manifest.source.clone(),
        $manifest: $manifest.clone(),
        candidate_set_revision: 3,
        advice_id: $advice.id.clone(),
    };
    let preservation = evaluate_scope_preservation(
        &Sha256ScopeDigest,
        &$manifest,
        &$advice,
        &disposition,
        &$observation,
    )
    .unwrap();
    let preservation_id = Uuid::new_v4();
    let preservation_input = ScopePreservationReceiptInput {
        receipt_id: preservation_id,
        request_id: Uuid::new_v4(),
        opportunity_id: $opportunity,
        candidate_set_id: $candidate,
        disposition_id: disposition.id,
        $observation: $observation.clone(),
        $result: preservation.clone(),
    };
    set_config(&$pool, $tenant, $workspace, false).await;
    let mut $unit = rw(&$store, &$enrollment.$auth, $tenant).await;
    assert_eq!(
        $unit.persist_scope_preservation_receipt($workspace, &preservation_input)
            .await,
        Err(Error::StaleContext)
    );
    drop($unit);
    set_config(&$pool, $tenant, $workspace, true).await;
    let mut $unit = rw(&$store, &$enrollment.$auth, $tenant).await;
    $unit.persist_scope_preservation_receipt($workspace, &preservation_input)
        .await
        .unwrap();
    $unit.commit().await.unwrap();
    let $audit_query = AdvisoryAuditQuery {
        limit: 1,
        scope_id: None,
        $after: None,
        capability: None,
        decision_point: None,
        $reason: None,
        $state: None,
    };
    let mut $unit = rw(&$store, &$enrollment.$auth, $tenant).await;
    let before_caller = $unit
        .candidate_advisory_opportunity_detail($workspace, $candidate, $opportunity)
        .await
        .unwrap()
        .$opportunity;
    assert_eq!(before_caller.guarded_advice_id, None);
    assert_eq!(
        before_caller.guarded_advice_digest.as_deref(),
        Some($advice.id.0.as_str())
    );
    assert_eq!(before_caller.disposition_id, Some(disposition.id));
    assert_eq!(before_caller.preservation_receipt_id, Some(preservation_id));
    assert_eq!(before_caller.preservation_status.as_deref(), Some("passed"));
    assert_eq!(before_caller.caller_receipt_id, None);
    assert_eq!(before_caller.verifier_receipt_id, None);
    drop($unit);
    let mut changed_preservation = preservation_input.clone();
    changed_preservation.disposition_id = Uuid::new_v4();
    let mut $unit = rw(&$store, &$enrollment.$auth, $tenant).await;
    assert_eq!(
        $unit.persist_scope_preservation_receipt($workspace, &changed_preservation)
            .await,
        Err(Error::InputConflict)
    );
    drop($unit);
    let caller_request = Uuid::new_v4();
    sqlx::query("INSERT INTO scope_candidate_receipts(tenant_id,workspace_id,candidate_set_id,operation,request_id,request_payload,result_revision,result_payload) VALUES($1,$2,$3,'save_review',$4,'{}',3,'{}')")
        .bind($tenant).bind($workspace).bind($candidate).bind(caller_request).execute(&$pool).await.unwrap();
    let caller = ScopeCallerLinkInput {
        link_id: Uuid::new_v4(),
        request_id: Uuid::new_v4(),
        opportunity_id: $opportunity,
        candidate_set_id: $candidate,
        disposition_id: disposition.id,
        preservation_receipt_id: preservation_id,
        caller_operation: "save_review".into(),
        caller_request_id: caller_request,
        caller_result_revision: 3,
        actor_id: $actor,
        session_id: $session,
    };
    set_config(&$pool, $tenant, $workspace, false).await;
    let mut $unit = rw(&$store, &$enrollment.$auth, $tenant).await;
    assert_eq!(
        $unit.link_scope_advisory_caller($workspace, &caller).await,
        Err(Error::StaleContext)
    );
    drop($unit);
    set_config(&$pool, $tenant, $workspace, true).await;
    let failed_preservation_id = Uuid::new_v4();
    let mut failed_result = preservation.clone();
    failed_result.status = ScopePreservationStatus::Failed;
    failed_result.reason_codes = vec!["source_changed".into()];
    sqlx::query("INSERT INTO advisory_scope_preservation_receipt(tenant_id,workspace_id,opportunity_id,candidate_set_id,receipt_id,request_id,advice_id,disposition_id,disposition_revision,source_digest,manifest_digest,eligible_set_digest,observed_candidate_set_revision,status,aggregate_schema,observation_payload,result_payload) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,3,'failed','tect.scope-preservation/1',$13,$14)")
        .bind($tenant).bind($workspace).bind($opportunity).bind($candidate).bind(failed_preservation_id)
        .bind(Uuid::new_v4()).bind(&$advice.id.0).bind(disposition.id).bind(disposition.revision)
        .bind(&$manifest.source.digest).bind(&$manifest.whole_set_digest).bind(&$manifest.eligible_set_digest)
        .bind(serde_json::to_value(&$observation).unwrap()).bind(serde_json::to_value(&failed_result).unwrap())
        .execute(&$pool).await.unwrap();
    let mut failed_caller = caller.clone();
    failed_caller.link_id = Uuid::new_v4();
    failed_caller.request_id = Uuid::new_v4();
    failed_caller.preservation_receipt_id = failed_preservation_id;
    let mut wrong_lineage = caller.clone();
    wrong_lineage.candidate_set_id = Uuid::new_v4();
    let mut $unit = rw(&$store, &$enrollment.$auth, $tenant).await;
    assert_eq!(
        $unit.link_scope_advisory_caller($workspace, &failed_caller)
            .await,
        Err(Error::InputConflict)
    );
    assert_eq!(
        $unit.link_scope_advisory_caller($workspace, &wrong_lineage)
            .await,
        Err(Error::InputConflict)
    );
    $unit.link_scope_advisory_caller($workspace, &caller)
        .await
        .unwrap();
    $unit.commit().await.unwrap();
    let mut $unit = rw(&$store, &$enrollment.$auth, $tenant).await;
    let before_verifier = $unit
        .candidate_advisory_opportunity_detail($workspace, $candidate, $opportunity)
        .await
        .unwrap()
        .$opportunity;
    assert_eq!(before_verifier.caller_receipt_id, Some(caller_request));
    assert_eq!(before_verifier.caller_link_id, Some(caller.link_id));
    assert_eq!(
        before_verifier.preservation_receipt_id,
        Some(preservation_id)
    );
    assert_eq!(before_verifier.verifier_receipt_id, None);
    drop($unit);
    let mut changed_caller = caller.clone();
    changed_caller.preservation_receipt_id = Uuid::new_v4();
    let mut $unit = rw(&$store, &$enrollment.$auth, $tenant).await;
    assert_eq!(
        $unit.link_scope_advisory_caller($workspace, &changed_caller)
            .await,
        Err(Error::InputConflict)
    );
    drop($unit);
    let mut verifier = ScopeVerifierReceiptInput {
        receipt_id: Uuid::new_v4(),
        request_id: Uuid::new_v4(),
        opportunity_id: $opportunity,
        candidate_set_id: $candidate,
        caller_link_id: caller.link_id,
        actor_id: $actor,
        session_id: $session,
        verified_revision: 3,
        verifier_digest: D.into(),
    };
    let mut $unit = rw(&$store, &$enrollment.$auth, $tenant).await;
    assert_eq!(
        $unit.persist_scope_verifier_receipt($workspace, &verifier)
            .await,
        Err(Error::InputConflict)
    );
    verifier.session_id = $verifier_session;
    $unit.persist_scope_verifier_receipt($workspace, &verifier)
        .await
        .unwrap();
    $unit.commit().await.unwrap();
    let skipped = Uuid::new_v4();
    sqlx::query("INSERT INTO advisory_opportunity(id,tenant_id,workspace_id,work_item_kind,work_item_id,source_revision,session_id,authorized_actor_id,capability,decision_point,config_revision,session_preference,request_preference,policy_version,request_key,material_digest,state,primary_reason) VALUES($1,$2,$3,'scope_candidate_set',$4,'3',$5,$6,'scope_decomposition','scope.decomposition.before_selection',1,'use_workspace','skip','1',$7,$8,'no_call','request_skip')")
        .bind(skipped).bind($tenant).bind($workspace).bind($candidate).bind($session).bind($actor)
        .bind(format!("skip-{skipped}")).bind(D).execute(&$pool).await.unwrap();
    let mut $unit = rw(&$store, &$enrollment.$auth, $tenant).await;
    let $first = $unit
        .candidate_advisory_audit($workspace, $candidate, &$audit_query)
        .await
        .unwrap();
    assert_eq!($first.opportunities.len(), 1);
    assert_eq!($first.opportunities[0].id, skipped);
    assert_eq!($first.opportunities[0].guarded_advice_digest, None);
    assert_eq!($first.opportunities[0].disposition_id, None);
    assert_eq!($first.opportunities[0].preservation_receipt_id, None);
    assert_eq!($first.opportunities[0].preservation_status, None);
    assert_eq!($first.opportunities[0].caller_receipt_id, None);
    assert_eq!($first.opportunities[0].caller_link_id, None);
    assert_eq!($first.opportunities[0].verifier_receipt_id, None);
    assert_eq!($first.opportunities[0].selected_save_observation, None);
    let mut $after = $first.next_after;
    let mut found = false;
    while let Some($cursor) = $after {
        let $page = $unit
            .candidate_advisory_audit(
                $workspace,
                $candidate,
                &AdvisoryAuditQuery {
                    $after: Some($cursor),
                    ..$audit_query.clone()
                },
            )
            .await
            .unwrap();
        if $page.opportunities[0].id == $opportunity {
            assert_eq!(
                $page.opportunities[0].caller_receipt_id,
                Some(caller_request)
            );
            assert_eq!($page.opportunities[0].caller_link_id, Some(caller.link_id));
            assert_eq!(
                $page.opportunities[0].verifier_receipt_id,
                Some(verifier.receipt_id)
            );
            assert_eq!(
                $page.opportunities[0].preservation_receipt_id,
                Some(preservation_id)
            );
            found = true;
        }
        $after = $page.next_after;
    }
    assert!(
        found,
        "selected opportunity must remain reachable through pagination"
    );
    drop($unit);
    let $foreign = admin::enroll_host(&$pool, None, vec![]).await.unwrap();
    assert_ne!($foreign.tenant_id, $tenant);
    let mut $foreign_unit = rw(&$store, &$foreign.$auth, $foreign.tenant_id).await;
    let foreign_page = $foreign_unit
        .candidate_advisory_audit($workspace, $candidate, &$audit_query)
        .await
        .unwrap();
    assert!(foreign_page.opportunities.is_empty());
    assert_eq!(foreign_page.aggregate.opportunities, 0);
    drop($foreign_unit);
    let completed_preselection: (bool, i64) = sqlx::query_as(
        "SELECT o.scope_id IS NULL,(SELECT count(*) FROM native_scopes n WHERE n.tenant_id=o.tenant_id AND n.workspace_id=o.workspace_id)::bigint FROM advisory_opportunity o WHERE o.id=$1",
    ).bind($opportunity).fetch_one(&$pool).await.unwrap();
    assert_eq!(completed_preselection, (true, 0));

    let original = serde_json::to_value(&$manifest).unwrap();
    sqlx::query("UPDATE advisory_scope_manifest SET aggregate_payload=jsonb_set(aggregate_payload,'{baseline_id}','\"tampered\"') WHERE opportunity_id=$1")
        .bind($opportunity).execute(&$pool).await.unwrap();
    let mut $unit = rw(&$store, &$enrollment.$auth, $tenant).await;
    assert!(
        $unit.scope_advisory_manifest($workspace, $opportunity)
            .await
            .is_err()
    );
    drop($unit);
    sqlx::query("UPDATE advisory_scope_manifest SET aggregate_payload=$1 WHERE opportunity_id=$2")
        .bind(original)
        .bind($opportunity)
        .execute(&$pool)
        .await
        .unwrap();
    };
}

pub(in super::super) use verify;
