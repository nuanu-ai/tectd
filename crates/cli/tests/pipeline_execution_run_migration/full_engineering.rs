use super::*;
use recovery_support::pipeline_reads::resolve_pipeline;

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn full_engineering_migrations_restart_without_rewriting_history() {
    let pool = PgPool::connect(&std::env::var("TECT_TEST_ADMIN_URL").expect("isolated admin URL"))
        .await
        .unwrap();
    let runtime_url = std::env::var("TECT_TEST_RUNTIME_URL").expect("isolated runtime URL");
    let role = std::env::var("TECT_TEST_RUNTIME_ROLE").expect("isolated role");
    admin::migrate(&pool, &role).await.unwrap();
    let temp = private_temp();
    let root = if std::env::var("TECT_TEST_KEEP_FAILURE_EVIDENCE").as_deref() == Ok("1") {
        temp.keep().canonicalize().unwrap()
    } else {
        temp.path().canonicalize().unwrap()
    };
    eprintln!("migration_fixture_root={}", root.display());
    let socket = root.join("full-migration.sock");
    let _daemon = Daemon::start(&tagged_url(&runtime_url, "full-migration"), socket.clone()).await;
    let enrollment = admin::enroll_host(&pool, None, vec![root.to_string_lossy().into_owned()])
        .await
        .unwrap();
    let config = root.join("host.json");
    host_file(&config, &enrollment.auth);
    for version in ["0.6.0-native.engineering.2", "0.6.0-native.engineering.3"] {
        let repo = root.join(format!("source-{}", version.chars().last().unwrap()));
        repository(&repo);
        let mut client = Mcp::start(
            &socket,
            &config,
            &Uuid::new_v4().to_string(),
            &format!("full-migration-{}", Uuid::new_v4()),
        )
        .await;
        let (source, candidate) = ready_source_candidate(&mut client, &repo).await;
        let opened = route(&mut client,"command","scope.open",json!({"request_id":Uuid::new_v4(),
            "candidate_set_id":source["candidate_set"]["id"],"candidate_set_revision":source["candidate_set"]["revision"],
            "candidate_snapshot_id":source["snapshot"]["id"],"candidate_id":candidate["id"],"candidate_revision":candidate["revision"]})).await;
        let mut draft = lifecycle_support::lightweight_draft();
        draft["nodes"][0]["pipeline"] = json!("slice.full-design-to-execution");
        draft["nodes"][0]["why_lightweight_insufficient"] =
            json!("Integrated cross-cutting contract and execution review require Full");
        draft["nodes"][0]["why_further_vertical_split_not_viable"] = json!(
            "One indivisible observable outcome shares exact acceptance and ownership boundaries"
        );
        let opened = ScopeOpenFixture::from_mutation(opened, "created");
        let planning = opened.read_planning(&mut client).await.value;
        let saved = save(&mut client, &planning, draft).await;
        let reviewed = review(&mut client, &saved).await;
        let slice = route(
            &mut client,
            "command",
            "slice.open",
            open_slice(&reviewed, &reviewed["draft"]["nodes"][0], Uuid::new_v4()),
        )
        .await["created"]
            .clone();
        let begun = route(&mut client,"command","slice.pipeline.begin",json!({"request_id":Uuid::new_v4(),"scope_id":reviewed["scope"]["id"],
            "slice_id":slice["id"],"slice_revision":slice["revision"],"definition_version":version,
            "qualification_reason":"Preserve one historical output across explicit successor migration"})).await;
        let begun = resolve_pipeline(&mut client, begun)
            .await
            .expect("resolve actual Full begin response");
        let (verdict, outcome, transition) = full_support::successful_route(&begun);
        let before = route(
            &mut client,
            "command",
            "slice.pipeline.phase.complete",
            full_support::completion(&begun, verdict, outcome, transition, None, None),
        )
        .await;
        let before = resolve_pipeline(&mut client, before)
            .await
            .expect("resolve actual Full completion response");
        assert_eq!(
            before.details_data()["outputs"].as_array().unwrap().len(),
            1
        );
        let output = &before.details_data()["outputs"][0];
        let predecessor_id = before.run()["id"]
            .as_str()
            .unwrap()
            .parse::<Uuid>()
            .unwrap();
        let stored_definition_before: Value =
            sqlx::query_scalar("SELECT definition FROM slice_pipeline_runs WHERE id=$1")
                .bind(predecessor_id)
                .fetch_one(&pool)
                .await
                .unwrap();
        let params = json!({"request_id":Uuid::new_v4(),"predecessor_run_id":before.run()["id"],"expected_revision":before.run()["revision"],
            "idempotency_key":format!("full-migration-{}",Uuid::new_v4()),"successor_definition_version":"0.6.0-native.engineering.4",
            "mappings":[{"legacy_obligation_id":output["phase_id"],"successor_obligation_id":output["phase_id"],
                "evidence_refs":[{"reference":output["id"],"digest":output["digest"]}]}]});
        assert_eq!(before.run()["revision"], 2);
        let stale_before = historical_seed::rows(&pool, predecessor_id).await;
        let mut stale = params.clone();
        stale["request_id"] = json!(Uuid::new_v4());
        stale["idempotency_key"] = json!(format!("stale-{}", Uuid::new_v4()));
        stale["expected_revision"] = json!(1);
        assert_eq!(
            route_error(&mut client, "command", "slice.pipeline.run.migrate", stale).await["error"]
                ["code"],
            "stale_revision"
        );
        assert_eq!(
            stale_before,
            historical_seed::rows(&pool, predecessor_id).await,
            "stale migration writes nothing"
        );
        let migrated = route(
            &mut client,
            "command",
            "slice.pipeline.run.migrate",
            params.clone(),
        )
        .await;
        let old = route(
            &mut client,
            "query",
            "slice.pipeline.context",
            json!({"run_id":before.run()["id"]}),
        )
        .await;
        let old = resolve_pipeline(&mut client, old)
            .await
            .expect("resolve actual old context response");
        assert_eq!(old.run()["status"], "superseded");
        assert_eq!(
            old.run()["revision"],
            before.run()["revision"].as_i64().unwrap() + 1
        );
        // Public definition delivery depends on current status/phase; compare the persisted immutable snapshot.
        let stored_definition_after: Value =
            sqlx::query_scalar("SELECT definition FROM slice_pipeline_runs WHERE id=$1")
                .bind(predecessor_id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(stored_definition_after, stored_definition_before);
        assert_eq!(
            old.run()["definition_version"],
            before.run()["definition_version"]
        );
        assert_eq!(
            old.run()["definition_digest"],
            before.run()["definition_digest"]
        );
        for field in ["outputs", "attempts", "bindings", "inputs"] {
            assert_eq!(
                old.details_data()[field],
                before.details_data()[field],
                "{version} {field}"
            );
        }
        let successor = route(
            &mut client,
            "query",
            "slice.pipeline.context",
            json!({"run_id":migrated["successor_run_id"]}),
        )
        .await;
        let successor = resolve_pipeline(&mut client, successor)
            .await
            .expect("resolve actual successor context response");
        assert_eq!(
            successor.run()["definition_version"],
            "0.6.0-native.engineering.4"
        );
        assert_eq!(
            successor.run()["definition_digest"],
            "85ec63bae1903fedb0c86ecd5326380ea8d524fe0ee29c5dce6e90b9a30cdd3d"
        );
        assert_eq!(successor.run()["current_phase_ordinal"], 1);
        assert_eq!(successor.run()["revision"], 1);
        assert_eq!(successor.run()["scope_id"], before.run()["scope_id"]);
        assert_eq!(successor.run()["slice_id"], before.run()["slice_id"]);
        for field in ["outputs", "attempts", "bindings", "inputs"] {
            assert!(
                successor.details_data()[field]
                    .as_array()
                    .unwrap()
                    .is_empty(),
                "{field}"
            );
        }
        assert_eq!(
            route(
                &mut client,
                "command",
                "slice.pipeline.run.migrate",
                params.clone()
            )
            .await["successor_run_id"],
            migrated["successor_run_id"]
        );
        let mut conflict = params.clone();
        conflict["mappings"][0]["successor_obligation_id"] = json!("changed");
        assert_eq!(
            route_error(
                &mut client,
                "command",
                "slice.pipeline.run.migrate",
                conflict
            )
            .await["error"]["code"],
            "input_conflict"
        );
        let superseded_before = historical_seed::rows(&pool, predecessor_id).await;
        let mut superseded = params;
        superseded["request_id"] = json!(Uuid::new_v4());
        superseded["idempotency_key"] = json!(format!("superseded-{}", Uuid::new_v4()));
        superseded["expected_revision"] = old.run()["revision"].clone();
        assert_eq!(
            route_error(
                &mut client,
                "command",
                "slice.pipeline.run.migrate",
                superseded
            )
            .await["error"]["code"],
            "forbidden"
        );
        assert_eq!(
            superseded_before,
            historical_seed::rows(&pool, predecessor_id).await,
            "superseded migration writes nothing"
        );
        let bypass = json!({"request_id":Uuid::new_v4(),"scope_id":before.run()["scope_id"],"slice_id":slice["id"],"slice_revision":slice["revision"],
            "outcome":"completed","summary":"Superseded history cannot be bypassed","evidence":[{"kind":"test","reference":"fixture","observation":"Remains managed"}],"scope_impact":"None","remaining_work":"Fresh successor phases"});
        assert_eq!(
            route_error(&mut client, "command", "slice.result.record", bypass).await["error"]["code"],
            "forbidden"
        );
        eprintln!(
            "full migration {version}: retained_output={} successor={} P1 rev1 replay/conflict/stale/manual_bypass passed",
            output["id"], migrated["successor_run_id"]
        );
    }
}
