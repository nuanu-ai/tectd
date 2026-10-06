#[tokio::test]
#[ignore = "requires exact disposable PG18.6 identity; synthetic approval CONTROL FIXTURE only"]
async fn technical_decision_comparison_pg_real_service_control_fixture() {
    let ControlFixture {
        admin_pool,
        runtime,
        owner,
        workspace,
        program,
        task,
        sessions,
        owner_context,
        compare_context,
        locator,
        operating,
        approved,
        technical_body,
        request,
        delegated,
    } = control_fixture().await;
    let guards = Arc::new(NoExternal);
    let base = WorkspaceService::new(
        Arc::new(PgStore::from_pool(runtime.clone())),
        guards.clone(),
        guards,
    );
    let whitelist = control_whitelist_json(&approved);
    let parsed_controls = parse_technical_decision_approvals(&whitelist).unwrap();
    assert_eq!(parsed_controls.len(), 1);
    let mut forged_config: Value = serde_json::from_str(&whitelist).unwrap();
    forged_config[0]["accepted"] = json!(true);
    assert!(matches!(
        parse_technical_decision_approvals(&serde_json::to_string(&forged_config).unwrap()),
        Err(Error::InvalidConfiguration)
    ));
    let trusted = service(&runtime, &operating, parsed_controls);
    let before = read_effect_counts(&admin_pool, owner.tenant_id).await;
    let TechnicalDeliveryMechanismRead::Compared(compared) = trusted
        .compare_technical_delivery_mechanisms(&compare_context, &request)
        .await
        .unwrap()
    else {
        panic!("current approved control must compare")
    };
    assert_eq!(compared.eligible_approach_ids, vec!["reuse"]);
    assert!(compared.needs_inspection.is_empty());
    assert_eq!(
        compared.assessments[0].source_support,
        TechnicalSourceSupport::Supported
    );
    assert_eq!(
        compared.assessments[1].source_support,
        TechnicalSourceSupport::Supported
    );
    assert_eq!(
        compared.assessments[0].engineering_adequacy,
        TechnicalAdequacy::Adequate
    );
    assert_eq!(
        compared.assessments[1].engineering_adequacy,
        TechnicalAdequacy::Dominated
    );
    assert_eq!(
        read_effect_counts(&admin_pool, owner.tenant_id).await,
        before,
        "successful read creates no persisted effect"
    );
    assert_eq!(
        base.compare_technical_delivery_mechanisms(&compare_context, &request)
            .await
            .unwrap(),
        TechnicalDeliveryMechanismRead::Unavailable
    );
    assert_eq!(
        service(&runtime, &operating, vec![])
            .compare_technical_delivery_mechanisms(&compare_context, &request)
            .await
            .unwrap(),
        TechnicalDeliveryMechanismRead::Unavailable
    );
    for mutation in 0..7 {
        let mut wrong = approved.clone();
        match mutation {
            0 => wrong.approval.owner_author_principal_id = delegated,
            1 => wrong.approval.recorded_by_principal_id = delegated,
            2 => wrong.binding.tenant_id = Uuid::new_v4(),
            3 => wrong.binding.workspace_id = Uuid::new_v4(),
            4 => wrong.binding.task_revision = 2,
            5 => wrong.reference.artifact_version = 2,
            6 => wrong.candidate_mapping[0].frozen_candidate.title = "forged".into(),
            _ => unreachable!(),
        }
        assert_eq!(
            service(&runtime, &operating, vec![wrong])
                .compare_technical_delivery_mechanisms(&compare_context, &request)
                .await
                .unwrap(),
            TechnicalDeliveryMechanismRead::Unavailable,
            "wrong control approval {mutation}"
        );
    }
    // Untrusted artifact fields never supply accepted validation or approval.
    // Each body receives its own synthetic whitelist SHA so parsing/binding
    // checks, rather than an absent whitelist entry, exercise the real resolver.
    for mutation in 0..9 {
        let mut value: Value = serde_json::from_str(&technical_body).unwrap();
        match mutation {
            0 => value["accepted"] = json!(true),
            1 => value["tenant_id"] = json!(Uuid::new_v4()),
            2 => value["workspace_id"] = json!(Uuid::new_v4()),
            3 => value["task_id"] = json!(Uuid::new_v4()),
            4 => value["task_revision"] = json!(2),
            5 => value["candidate_mapping"][0]["frozen_candidate"]["title"] = json!("forged"),
            6 => value["facts"][0]["observed_at"] = json!(now() + 3600),
            7 => value["facts"][0]["expires_at"] = json!(now() - 1),
            8 => {}
            _ => unreachable!(),
        }
        let mutated = if mutation == 8 {
            format!(
                "{{\"schema\":\"{}\",{}",
                TECHNICAL_EVIDENCE_SCHEMA,
                &technical_body[1..]
            )
        } else {
            serde_json::to_string(&value).unwrap()
        };
        let mut control = approved.clone();
        control.reference.artifact_id = Uuid::new_v4();
        control.reference.content_sha256 = sha(mutated.as_bytes());
        artifact(
            &admin_pool,
            owner.tenant_id,
            workspace,
            control.reference.artifact_id,
            &mutated,
            TECHNICAL_EVIDENCE_FORMAT,
        )
        .await;
        let mut mutated_request = request.clone();
        mutated_request.evidence_reference = control.reference.clone();
        let result = service(&runtime, &operating, vec![control])
            .compare_technical_delivery_mechanisms(&compare_context, &mutated_request)
            .await;
        if matches!(mutation, 0 | 8) {
            assert_eq!(result, Err(Error::Forbidden), "artifact shape {mutation}");
        } else {
            assert_eq!(
                result,
                Ok(TechnicalDeliveryMechanismRead::Unavailable),
                "artifact binding/time {mutation}"
            );
            // SQLx Drop queues rollback for resolver errors; synchronize the
            // next independent negative test on actual lock release readiness.
            declaration_lock_released(&admin_pool, owner.tenant_id, workspace, program).await;
        }
    }
    // Ready metadata and an approved digest cannot authenticate changed bytes.
    let mut wrong_bytes = approved.clone();
    wrong_bytes.reference.artifact_id = Uuid::new_v4();
    artifact(
        &admin_pool,
        owner.tenant_id,
        workspace,
        wrong_bytes.reference.artifact_id,
        "{}",
        TECHNICAL_EVIDENCE_FORMAT,
    )
    .await;
    let mut byte_request = request.clone();
    byte_request.evidence_reference = wrong_bytes.reference.clone();
    assert_eq!(
        service(&runtime, &operating, vec![wrong_bytes])
            .compare_technical_delivery_mechanisms(&compare_context, &byte_request)
            .await
            .unwrap(),
        TechnicalDeliveryMechanismRead::Unavailable
    );
    let mut wrong_operating = operating.clone();
    wrong_operating.sha256 = "f".repeat(64);
    assert_eq!(
        service(&runtime, &wrong_operating, vec![approved.clone()])
            .compare_technical_delivery_mechanisms(&compare_context, &request)
            .await
            .unwrap(),
        TechnicalDeliveryMechanismRead::Unavailable
    );
    let mut stale = request.clone();
    stale.expected_task_revision = 2;
    assert_eq!(
        trusted
            .compare_technical_delivery_mechanisms(&compare_context, &stale)
            .await,
        Err(Error::StaleRevision)
    );
    let mut missing = compare_context.clone();
    missing.native_session_id = Uuid::new_v4().to_string();
    assert_eq!(
        trusted
            .compare_technical_delivery_mechanisms(&missing, &request)
            .await,
        Err(Error::WorkspaceNotOpen)
    );
    let mut foreign = compare_context.clone();
    foreign.workspace_key = format!("foreign-{}", Uuid::new_v4());
    assert_eq!(
        trusted
            .compare_technical_delivery_mechanisms(&foreign, &request)
            .await,
        Err(Error::SessionWorkspaceMismatch)
    );
    sqlx::query("UPDATE agent_sessions SET revoked=true WHERE id=$1")
        .bind(sessions[1])
        .execute(&admin_pool)
        .await
        .unwrap();
    assert_eq!(
        trusted
            .compare_technical_delivery_mechanisms(&compare_context, &request)
            .await,
        Err(Error::SessionRevoked)
    );
    sqlx::query("UPDATE agent_sessions SET revoked=false WHERE id=$1")
        .bind(sessions[1])
        .execute(&admin_pool)
        .await
        .unwrap();

    // Deterministic held locks exercise public comparison, then fresh retry.
    for held_task in [true, false] {
        let store = PgStore::from_pool(runtime.clone());
        let mut holder = store.begin(TransactionMode::ReadWrite).await.unwrap();
        holder.authenticate(&owner.auth).await.unwrap();
        holder.set_tenant(owner.tenant_id).await.unwrap();
        if held_task {
            holder
                .lock_matrix_task(workspace, task)
                .await
                .unwrap()
                .unwrap();
        } else {
            holder
                .matrix_requirements_context_store()
                .unwrap()
                .lock_matrix_requirements_head(
                    workspace,
                    RequirementsAnchor::Program {
                        program_id: program,
                    },
                )
                .await
                .unwrap();
        }
        assert_eq!(
            tokio::time::timeout(
                Duration::from_secs(2),
                trusted.compare_technical_delivery_mechanisms(&compare_context, &request)
            )
            .await
            .unwrap(),
            Err(Error::StaleRevision)
        );
        // Losing comparison must release the other lock before holder finishes.
        if held_task {
            holder
                .matrix_requirements_context_store()
                .unwrap()
                .lock_matrix_requirements_head(
                    workspace,
                    RequirementsAnchor::Program {
                        program_id: program,
                    },
                )
                .await
                .unwrap();
        } else {
            holder
                .lock_matrix_task(workspace, task)
                .await
                .unwrap()
                .unwrap();
        }
        holder.commit().await.unwrap();
        assert!(matches!(
            trusted
                .compare_technical_delivery_mechanisms(&compare_context, &request)
                .await
                .unwrap(),
            TechnicalDeliveryMechanismRead::Compared(_)
        ));
    }
    // A subsequent accepted declaration makes the old frozen source stale.
    let changed = base
        .propose_matrix_requirements_context(
            &owner_context,
            &ProposeMatrixRequirementsContext {
                request_id: Uuid::new_v4(),
                locator: locator.clone(),
                expected_context_revision: 1,
                patches: vec![RequirementDeclarationPatch::Set {
                    value: DeclaredRequirementValue::PromisedProof("changed proof".into()),
                }],
            },
        )
        .await
        .unwrap();
    base.confirm_matrix_requirements_context(
        &owner_context,
        &ConfirmMatrixRequirementsContext {
            request_id: Uuid::new_v4(),
            locator,
            proposal_revision: 2,
            proposal_digest: changed.proposal.digest().into(),
            owner_response_ref: "SYNTHETIC CHANGED DECLARATION CONTROL".into(),
        },
    )
    .await
    .unwrap();
    assert_eq!(
        trusted
            .compare_technical_delivery_mechanisms(&compare_context, &request)
            .await,
        Err(Error::StaleRevision)
    );
    let counts:(i64,i64,i64)=sqlx::query_as("SELECT (SELECT count(*) FROM advisory_dispatch WHERE tenant_id=$1),(SELECT count(*) FROM advisory_opportunity WHERE tenant_id=$1),(SELECT count(*) FROM matrix_task_revisions WHERE tenant_id=$1)")
        .bind(owner.tenant_id).fetch_one(&admin_pool).await.unwrap();
    assert_eq!(counts, (0, 0, 1));
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM matrix_tasks WHERE id=$1")
            .bind(task)
            .fetch_one(&runtime)
            .await
            .unwrap(),
        0
    );
    eprintln!(
        "S02 real authenticated PG service/operating validator/technical resolver positive, control delegated authority, denials, public task/declaration contention and stale declaration proof passed; synthetic controls are NOT genuine owner acceptance"
    );
}
