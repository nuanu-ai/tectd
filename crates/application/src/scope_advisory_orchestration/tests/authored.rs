use super::*;

#[tokio::test]
async fn authored_supply_receives_authority_and_exact_request_without_legacy_fallback() {
    let candidate_set_id = Uuid::from_u128(5);
    let source = authored_source(candidate_set_id, 7);
    let observation = crate::ScopeAuthorityObservation {
        workspace_id: Uuid::from_u128(1),
        actor_id: Uuid::from_u128(2),
        session_id: Uuid::from_u128(3),
        candidate_set_id,
        source: source.clone(),
        obligations: vec![SourceObligation {
            id: "obligation.intent".into(),
            source_input_id: "program.intent".into(),
            statement_digest: "a".repeat(64),
            conditions: vec![],
            exceptions: vec![],
        }],
    };
    let manifest = authored_manifest(source);
    let authored_calls = Arc::new(AtomicUsize::new(0));
    let legacy_calls = Arc::new(AtomicUsize::new(0));
    let seen_authored = Arc::new(std::sync::Mutex::new(None));
    let supplier = RecordingManifestSupplier {
        authored_calls: authored_calls.clone(),
        legacy_calls: legacy_calls.clone(),
        seen_authored: seen_authored.clone(),
        manifest: manifest.clone(),
        fail_authored: false,
    };
    let request = RunScopeAdvisory {
        request_id: Uuid::from_u128(6),
        candidate_set_id,
        session_preference: AdvisoryRequestPreference::UseWorkspace,
        request_preference: AdvisoryRequestPreference::UseWorkspace,
        authored_scope_set: Some(authored_set()),
    };

    assert_eq!(
        supply_scope_manifest(&supplier, Uuid::from_u128(9), &observation, &request).await,
        Ok(manifest)
    );
    assert_eq!(authored_calls.load(Ordering::SeqCst), 1);
    assert_eq!(legacy_calls.load(Ordering::SeqCst), 0);
    let seen = seen_authored.lock().unwrap().clone().unwrap();
    assert_eq!(seen.tenant_id, Uuid::from_u128(9));
    assert_eq!(seen.observation, observation);
    assert_eq!(seen.authored_scope_set, request.authored_scope_set.unwrap());
}

#[tokio::test]
async fn authored_revision_mismatch_and_supplier_failure_fail_closed() {
    let candidate_set_id = Uuid::from_u128(5);
    let source = authored_source(candidate_set_id, 8);
    let observation = crate::ScopeAuthorityObservation {
        workspace_id: Uuid::from_u128(1),
        actor_id: Uuid::from_u128(2),
        session_id: Uuid::from_u128(3),
        candidate_set_id,
        source: source.clone(),
        obligations: vec![SourceObligation {
            id: "obligation.intent".into(),
            source_input_id: "program.intent".into(),
            statement_digest: "a".repeat(64),
            conditions: vec![],
            exceptions: vec![],
        }],
    };
    let authored_calls = Arc::new(AtomicUsize::new(0));
    let legacy_calls = Arc::new(AtomicUsize::new(0));
    let supplier = RecordingManifestSupplier {
        authored_calls: authored_calls.clone(),
        legacy_calls: legacy_calls.clone(),
        seen_authored: Arc::new(std::sync::Mutex::new(None)),
        manifest: authored_manifest(source),
        fail_authored: false,
    };
    let request = RunScopeAdvisory {
        request_id: Uuid::from_u128(6),
        candidate_set_id,
        session_preference: AdvisoryRequestPreference::UseWorkspace,
        request_preference: AdvisoryRequestPreference::UseWorkspace,
        authored_scope_set: Some(authored_set()),
    };
    assert_eq!(
        supply_scope_manifest(&supplier, Uuid::from_u128(9), &observation, &request).await,
        Err(Error::StaleRevision)
    );
    assert_eq!(authored_calls.load(Ordering::SeqCst), 0);

    let failing = RecordingManifestSupplier {
        authored_calls: authored_calls.clone(),
        legacy_calls: legacy_calls.clone(),
        seen_authored: supplier.seen_authored.clone(),
        manifest: supplier.manifest.clone(),
        fail_authored: true,
    };
    let mut matching_request = request;
    matching_request
        .authored_scope_set
        .as_mut()
        .unwrap()
        .expected_candidate_set_revision = 8;
    assert_eq!(
        supply_scope_manifest(
            &failing,
            Uuid::from_u128(9),
            &observation,
            &matching_request
        )
        .await,
        Err(Error::InputPending)
    );
    assert_eq!(authored_calls.load(Ordering::SeqCst), 1);
    assert_eq!(legacy_calls.load(Ordering::SeqCst), 0);
}

#[test]
fn authored_manifest_replay_binds_request_digest_and_all_persisted_identity() {
    let candidate_set_id = Uuid::from_u128(5);
    let manifest = authored_manifest(authored_source(candidate_set_id, 7));
    let config = no_call_config(WorkspaceAdvisoryMode::Optional);
    let request = RunScopeAdvisory {
        request_id: Uuid::from_u128(6),
        candidate_set_id,
        session_preference: AdvisoryRequestPreference::UseWorkspace,
        request_preference: AdvisoryRequestPreference::UseWorkspace,
        authored_scope_set: Some(authored_set()),
    };
    let digest = authored_request_digest(request.authored_scope_set.as_ref().unwrap()).unwrap();
    let opportunity = opportunity_for_authored_manifest(&request, &config, &manifest);
    let record = ScopeManifestRecord {
        opportunity_id: opportunity.id,
        candidate_set_id,
        config_revision: config.revision,
        opportunity_material_digest: manifest.whole_set_digest.clone(),
        manifest,
    };
    let stored = crate::StoredScopeManifestRecord {
        record,
        authored_request_digest: Some(digest.clone()),
    };
    assert!(
        validate_authored_replay_binding(
            &stored,
            Some(&opportunity),
            &request,
            &config,
            Uuid::from_u128(42),
            Uuid::from_u128(41),
            &digest,
        )
        .is_ok()
    );
    assert_eq!(
        validate_authored_replay_binding(
            &stored,
            Some(&opportunity),
            &request,
            &config,
            Uuid::from_u128(42),
            Uuid::from_u128(41),
            &"b".repeat(64),
        ),
        Err(Error::InputConflict)
    );
    let mut legacy = stored;
    legacy.authored_request_digest = None;
    assert_eq!(
        validate_authored_replay_binding(
            &legacy,
            Some(&opportunity),
            &request,
            &config,
            Uuid::from_u128(42),
            Uuid::from_u128(41),
            &digest,
        ),
        Err(Error::InputConflict)
    );
}
