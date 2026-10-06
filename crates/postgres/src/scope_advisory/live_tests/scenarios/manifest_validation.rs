macro_rules! verify {
    ($altered:ident, $auth:ident, $candidate:ident, $enrollment:ident, $manifest:ident, $missing:ident, $opportunity:ident, $prepared:ident, $program:ident, $rejected:ident, $replay:ident, $request_key:ident, $snapshot:ident, $source_refs:ident, $store:ident, $stored:ident, $tenant:ident, $unit:ident, $workspace:ident $(,)?) => {
        let $prepared = ScopeManifestRecord {
            opportunity_id: $opportunity,
            candidate_set_id: $candidate,
            config_revision: 1,
            opportunity_material_digest: D.into(),
            $manifest: $manifest.clone(),
        };
        let mut $unit = rw(&$store, &$enrollment.$auth, $tenant).await;
        assert!(
            $unit
                .scope_advisory_manifest_by_request_key($workspace, &$request_key)
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            $unit
                .scope_advisory_manifest_by_request_key(Uuid::new_v4(), &$request_key)
                .await
                .unwrap()
                .is_none()
        );
        let wrong = ScopeManifestRecord {
            opportunity_id: $opportunity,
            candidate_set_id: Uuid::new_v4(),
            config_revision: 1,
            opportunity_material_digest: D.into(),
            $manifest: $manifest.clone(),
        };
        assert_eq!(
            $unit
                .prepare_scope_advisory_manifest($workspace, &wrong)
                .await,
            Err(Error::InputConflict)
        );
        let mut rejected_baseline = $manifest.clone();
        let mut $rejected = rejected_baseline.emitted[0].clone();
        $rejected.material.candidates[0].title = "Rejected".into();
        $rejected.material_digest =
            scope_candidate_material_digest(&Sha256ScopeDigest, &$rejected.material).unwrap();
        $rejected.id = stable_scope_alternative_id(
            &Sha256ScopeDigest,
            &rejected_baseline.constructor,
            &rejected_baseline.source.digest,
            $rejected.kind,
            &$rejected.material_digest,
            &$rejected.coverage,
        )
        .unwrap();
        rejected_baseline.$rejected = vec![RejectedScopeAlternative {
            alternative: $rejected.clone(),
            reason_codes: vec!["not_eligible".into()],
        }];
        rejected_baseline.baseline_id = $rejected.id.clone();
        rejected_baseline.ordered_ids.push($rejected.id);
        rejected_baseline.ordered_ids.sort();
        rejected_baseline.whole_set_digest = rejected_baseline
            .canonical_whole_set_digest(&Sha256ScopeDigest)
            .unwrap();
        let rejected_record = ScopeManifestRecord {
            opportunity_id: $opportunity,
            candidate_set_id: $candidate,
            config_revision: 1,
            opportunity_material_digest: D.into(),
            $manifest: rejected_baseline,
        };
        assert_eq!(
            $unit
                .prepare_scope_advisory_manifest($workspace, &rejected_record)
                .await,
            Err(Error::InvalidArguments)
        );
        let make_record = |$manifest: ScopeConstructorManifest| ScopeManifestRecord {
            opportunity_id: $opportunity,
            candidate_set_id: $candidate,
            config_revision: 1,
            opportunity_material_digest: D.into(),
            $manifest,
        };
        let $missing = super::live_support::$manifest(
            $candidate,
            $snapshot,
            $program,
            &[($source_refs[0], D)],
        );
        assert_eq!(
            $unit
                .prepare_scope_advisory_manifest($workspace, &make_record($missing))
                .await,
            Err(Error::InvalidSource)
        );
        let extra = super::live_support::$manifest(
            $candidate,
            $snapshot,
            $program,
            &[
                ($source_refs[0], D),
                ($source_refs[1], D),
                (Uuid::new_v4(), D),
            ],
        );
        assert_eq!(
            $unit
                .prepare_scope_advisory_manifest($workspace, &make_record(extra))
                .await,
            Err(Error::InvalidSource)
        );
        let different_digest = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
        let $altered = super::live_support::$manifest(
            $candidate,
            $snapshot,
            $program,
            &[($source_refs[0], D), ($source_refs[1], different_digest)],
        );
        assert_eq!(
            $unit
                .prepare_scope_advisory_manifest($workspace, &make_record($altered))
                .await,
            Err(Error::InvalidSource)
        );
        let mut omitted_coverage = $manifest.clone();
        omitted_coverage.emitted[0].coverage.pop();
        reseal_manifest(&mut omitted_coverage);
        assert_eq!(
            $unit
                .prepare_scope_advisory_manifest($workspace, &make_record(omitted_coverage))
                .await,
            Err(Error::InvalidSource)
        );
        let mut stale_snapshot = $manifest.clone();
        stale_snapshot.source.snapshot_id = Uuid::new_v4();
        reseal_manifest(&mut stale_snapshot);
        assert_eq!(
            $unit
                .prepare_scope_advisory_manifest($workspace, &make_record(stale_snapshot))
                .await,
            Err(Error::StaleRevision)
        );
        let authored_request_digest = "a".repeat(64);
        $unit
            .prepare_authored_scope_advisory_manifest(
                $workspace,
                &$prepared,
                &authored_request_digest,
            )
            .await
            .unwrap();
        let $stored = $unit
            .scope_advisory_manifest_by_request_key($workspace, &$request_key)
            .await
            .unwrap()
            .unwrap();
        assert_eq!($stored.record, $prepared);
        assert_eq!(
            $stored.authored_request_digest.as_deref(),
            Some(authored_request_digest.as_str())
        );
        $unit.commit().await.unwrap();
        let mut $replay = rw(&$store, &$enrollment.$auth, $tenant).await;
        let stored_replay = $replay
            .scope_advisory_manifest_by_request_key($workspace, &$request_key)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(stored_replay, $stored);
        assert_eq!(
            $replay
                .prepare_authored_scope_advisory_manifest($workspace, &$prepared, &"b".repeat(64),)
                .await,
            Err(Error::InputConflict)
        );
        $replay.commit().await.unwrap();
    };
}

pub(in super::super) use verify;
