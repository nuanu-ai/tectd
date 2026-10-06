macro_rules! verify {
    ($advice:ident, $auth:ident, $authored_scope_set:ident, $authority:ident, $authority_request:ident, $candidate:ident, $enrollment:ident, $opportunity:ident, $pool:ident, $replay:ident, $session:ident, $started:ident, $state:ident, $store:ident, $tenant:ident, $workspace:ident $(,)?) => {

    // Explicit test-only opt-in crosses the real Jev HTTP adapter and the
    // committed dispatch lifecycle. The same request is replayed while the
    // listener is open so an accidental retry is visible on the socket.
    // Preceding missing-policy/denied cases use the original deny-all store.
    let (signed_policy, owner_keys) =
        signed_scope_budget_fixture($workspace, $enrollment.principal_id);
    let positive_store = $store.clone().with_budget_owner_keys(owner_keys);
    let mut policy_unit = rw(&positive_store, &$enrollment.$auth, $tenant).await;
    policy_unit
        .advisory_budget_policy_store()
        .unwrap()
        .install_budget_policy($workspace, &signed_policy)
        .await
        .unwrap();
    policy_unit.commit().await.unwrap();
    let verified_now = i64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis(),
    )
    .unwrap();
    let mut untrusted_unit = rw(&$store, &$enrollment.$auth, $tenant).await;
    assert_eq!(
        untrusted_unit
            .advisory_budget_policy_store()
            .unwrap()
            .candidate_budget_policy($workspace, verified_now)
            .await
            .unwrap(),
        Some(signed_policy.clone())
    );
    assert_eq!(
        untrusted_unit
            .advisory_budget_policy_store()
            .unwrap()
            .authorized_budget_policy($workspace, verified_now)
            .await
            .unwrap(),
        None
    );
    untrusted_unit.commit().await.unwrap();
    let mut verified_unit = rw(&positive_store, &$enrollment.$auth, $tenant).await;
    let verified_policy = verified_unit
        .advisory_budget_policy_store()
        .unwrap()
        .authorized_budget_policy($workspace, verified_now)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(verified_policy.id(), signed_policy.id());
    assert_eq!(verified_policy.version(), signed_policy.version());
    assert_eq!(verified_policy.digest(), signed_policy.digest());
    assert_eq!(verified_policy, signed_policy);
    assert!(verified_policy.is_effective_at(verified_now));
    verified_unit.commit().await.unwrap();

    let (endpoint, fake_done, fake_server) = fake_jev_once().await;
    let jev = tect_host::JevScopeAdviceProvider::new(
        tect_host::JevScopeAdviceConfig {
            profile: "fixture".into(),
            endpoint: endpoint.clone(),
            model: "jev".into(),
            timeout: std::time::Duration::from_secs(2),
            maximum_request_bytes: 65_536,
            maximum_response_bytes: 65_536,
        },
        "test-only-credential".into(),
    )
    .unwrap();
    let positive_service = WorkspaceService::new_with_scope_advisory_adapters(
        std::sync::Arc::new(positive_store.clone()),
        std::sync::Arc::new(UnusedHostAdapters),
        std::sync::Arc::new(UnusedHostAdapters),
        std::sync::Arc::new(PgScopeAuthorityObserver::new(
            positive_store.clone(),
            std::sync::Arc::new(FixtureCandidateGuidance),
        )),
        std::sync::Arc::new(PgScopeAuthoredManifestSupplier::new(
            positive_store.clone(),
            std::sync::Arc::new(PgScopeAuthorityObserver::new(
                positive_store.clone(),
                std::sync::Arc::new(FixtureCandidateGuidance),
            )),
        )),
        std::sync::Arc::new(tect_application::SignedScopeBudgetPreflight),
        std::sync::Arc::new(jev),
    );
    let positive_context = tect_domain::RequestContext {
        $auth: $enrollment.$auth.clone(),
        native_session_id: $session.to_string(),
        workspace_key: format!("scope-live-{workspace}", $workspace = $workspace),
    };
    let positive_request = tect_application::RunScopeAdvisory {
        request_id: Uuid::new_v4(),
        candidate_set_id: $candidate,
        session_preference: tect_domain::AdvisoryRequestPreference::UseWorkspace,
        request_preference: tect_domain::AdvisoryRequestPreference::UseWorkspace,
        $authored_scope_set: Some($authored_scope_set.clone()),
    };
    let positive = positive_service
        .run_scope_advisory(&positive_context, &positive_request)
        .await
        .unwrap();
    assert_eq!(
        positive.$opportunity.$state,
        AdvisoryOpportunityState::Advised
    );
    assert!(positive.$opportunity.provider_called);
    assert_eq!(
        positive.$opportunity.primary_reason,
        AdvisoryReason::ProviderResponse
    );
    assert!(positive.$advice.is_some());
    let $replay = positive_service
        .run_scope_advisory(&positive_context, &positive_request)
        .await
        .unwrap();
    assert_eq!($replay.$opportunity.id, positive.$opportunity.id);
    assert_eq!($replay.$opportunity.$state, positive.$opportunity.$state);
    assert_eq!($replay.$advice, positive.$advice);
    assert_eq!(
        $replay.$opportunity.provider_called,
        positive.$opportunity.provider_called
    );
    fake_done.send(()).unwrap();
    let (received_body, second_call) = fake_server.await.unwrap();
    assert!(!second_call, "replay sent a second HTTP request");
    let sent: serde_json::Value = serde_json::from_slice(&received_body).unwrap();
    assert_eq!(
        sent.as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect::<Vec<_>>(),
        ["model", "questions", "state"]
    );
    let emitted = sent["state"]["emitted"].as_array().unwrap();
    let bound = sent["state"]["request"]["alternatives"].as_array().unwrap();
    assert!(!emitted.is_empty());
    assert_eq!(emitted.len(), bound.len());
    assert!(sent["state"].get("rejected").is_none());
    for (material, alternative) in emitted.iter().zip(bound) {
        assert_eq!(material["id"], alternative["id"]);
        assert_eq!(material["material_digest"], alternative["material_digest"]);
        assert_eq!(material["kind"], alternative["kind"]);
        assert_eq!(material["material"]["boundary"], "finite");
        assert_eq!(material["material"]["candidates"][0]["title"], "Cohesive");
    }
    let dispatch_rows: Vec<(i32, String, String, String, String, String, String, serde_json::Value, Vec<u8>, String, String, Option<String>, bool, bool)> = sqlx::query_as(
        "SELECT attempt_number,provider,model,state,send_certainty,outcome,retry_basis,configuration_snapshot,request_payload,payload_digest,material_digest,raw_response_ref,send_started_at IS NOT NULL,sealed_at IS NOT NULL FROM advisory_dispatch WHERE tenant_id=$1 AND workspace_id=$2 AND opportunity_id=$3 ORDER BY attempt_number",
    )
    .bind($tenant).bind($workspace).bind(positive.$opportunity.id)
    .fetch_all(&$pool).await.unwrap();
    assert_eq!(dispatch_rows.len(), 1);
    let (
        attempt,
        provider_name,
        model,
        $state,
        certainty,
        outcome,
        retry_basis,
        config_snapshot,
        request_payload,
        payload_digest,
        material_digest,
        raw_ref,
        $started,
        sealed,
    ) = &dispatch_rows[0];
    assert_eq!(
        (*attempt, provider_name.as_str(), model.as_str()),
        (1, "jev-system-one", "jev")
    );
    assert_eq!(
        (
            $state.as_str(),
            certainty.as_str(),
            outcome.as_str(),
            retry_basis.as_str()
        ),
        ("sealed", "sent", "provider_response", "initial")
    );
    assert!(*$started && *sealed);
    assert_eq!(
        config_snapshot["budget_policy_id"],
        signed_policy.id().to_string()
    );
    assert_eq!(config_snapshot["budget_policy"], serde_json::json!({
        "policy_id": signed_policy.id().to_string(),
        "policy_version": signed_policy.version(),
        "policy_digest": signed_policy.digest(),
    }));
    assert_eq!(config_snapshot["request_body_length"], received_body.len());
    assert_eq!(config_snapshot["request_body_sha256"], format!("{:x}", sha2::Sha256::digest(&received_body)));
    assert_eq!(config_snapshot["destination"], endpoint.as_str());
    assert_eq!(config_snapshot["wire_version"], "jev-system-one-json/3");
    assert_eq!(request_payload, &received_body);
    assert_eq!(
        payload_digest,
        &format!("{:x}", sha2::Sha256::digest(&received_body))
    );
    assert_eq!(material_digest, &positive.$opportunity.material_digest);
    assert!(raw_ref.as_deref().unwrap().contains("jev:fixture:"));
    let reservation_audit: (i64, bool) = sqlx::query_as(
        "SELECT count(*)::bigint,COALESCE(bool_and(r.policy_id=$4 AND r.policy_version=$5 AND r.policy_digest=$6 AND r.policy_effective_from_unix_ms=$7 AND r.policy_effective_until_unix_ms=$8 AND r.request_sha256=d.payload_digest AND r.request_utf8_bytes=octet_length(d.request_payload) AND r.reserved_calls=1 AND r.reserved_retry_dispatches=0 AND d.attempt_number=1 AND d.state='sealed' AND p.version=r.policy_version AND p.digest=r.policy_digest AND p.effective_from_unix_ms=r.policy_effective_from_unix_ms AND p.effective_until_unix_ms=r.policy_effective_until_unix_ms),false) FROM advisory_budget_reservations r JOIN advisory_dispatch d ON (d.tenant_id,d.workspace_id,d.opportunity_id,d.id)=(r.tenant_id,r.workspace_id,r.opportunity_id,r.dispatch_id) JOIN advisory_budget_policies p ON (p.tenant_id,p.workspace_id,p.id)=(r.tenant_id,r.workspace_id,r.policy_id) WHERE r.tenant_id=$1 AND r.workspace_id=$2 AND r.opportunity_id=$3",
    )
    .bind($tenant).bind($workspace).bind(positive.$opportunity.id)
    .bind(signed_policy.id()).bind(signed_policy.version()).bind(signed_policy.digest())
    .bind(signed_policy.effective_from_unix_ms()).bind(signed_policy.effective_until_unix_ms())
    .fetch_one(&$pool).await.unwrap();
    assert_eq!(reservation_audit, (1, true));
    let receipt: (Vec<u8>, Option<i64>, Option<i64>) = sqlx::query_as(
        "SELECT response_payload,input_tokens,output_tokens FROM advisory_dispatch WHERE opportunity_id=$1",
    ).bind(positive.$opportunity.id).fetch_one(&$pool).await.unwrap();
    assert_eq!(receipt.1, Some(11));
    assert_eq!(receipt.2, Some(5));
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&receipt.0).unwrap()["model"],
        "jev"
    );
    let audit: (String, String, i64) = sqlx::query_as(
        "SELECT state,primary_reason,(SELECT count(*) FROM advisory_dispatch d WHERE d.opportunity_id=o.id)::bigint FROM advisory_opportunity o WHERE id=$1",
    ).bind(positive.$opportunity.id).fetch_one(&$pool).await.unwrap();
    assert_eq!(audit, ("advised".into(), "provider_response".into(), 1));
    let attribution: (String, String, String, Uuid, String) = sqlx::query_as(
        "SELECT capability,decision_point,work_item_kind,work_item_id,source_revision FROM advisory_opportunity WHERE id=$1",
    ).bind(positive.$opportunity.id).fetch_one(&$pool).await.unwrap();
    assert_eq!(
        attribution,
        (
            "scope_decomposition".into(),
            "scope.decomposition.before_selection".into(),
            "scope_candidate_set".into(),
            $candidate,
            "3".into(),
        )
    );

    assert_eq!(
        $authority
            .observe(&ScopeAuthorityRequest {
                actor_id: Uuid::new_v4(),
                ..$authority_request
            })
            .await,
        Err(Error::Forbidden)
    );
    };
}

pub(in super::super) use verify;
