use super::*;
fn value() -> KnowledgeUnitRevision {
    value_in(Uuid::new_v4(), Uuid::new_v4())
}
fn value_in(tenant: Uuid, workspace: Uuid) -> KnowledgeUnitRevision {
    let unit = Uuid::new_v4();
    let event = Uuid::new_v4();
    let refs = refs(tenant, workspace, unit, 1, event);
    KnowledgeUnitRevision {
        unit_id: unit,
        revision: 1,
        active: true,
        constraint: KnowledgeConstraintDraft {
            title: "title".into(),
            statement: "statement".into(),
            modality: KnowledgeModality::Must,
            action: "act".into(),
            target_iri: "urn:target".into(),
            conditions: vec!["condition".into()],
            exceptions: vec!["exception".into()],
            source: KnowledgeSourceSnapshot {
                title: "source".into(),
                uri: "urn:source".into(),
                text: "source text".into(),
            },
            binding: KnowledgeBinding::Workspace,
            purpose: KnowledgePurpose::ExecutionConstraint,
            version_resolution: KnowledgeVersionResolution::CurrentAccepted,
        },
        source_sha256: sha256(b"source text"),
        rdf_digest: "digest".into(),
        rdf_digest_method: "rdfc-1.0-sha256".into(),
        rdf_digest_scope: KnowledgeRdfDigestScope::RevisionPublicationPayload,
        publication_event_id: event,
        unit_iri: refs.unit,
        revision_iri: refs.revision,
        source_iri: refs.source,
        publication_event_iri: refs.event,
        publication_operation: KnowledgeOperation::Create,
        publication_reason: "reason".into(),
        publication_authority_basis: "authority".into(),
        publication_actor_principal_id: Uuid::new_v4(),
        publication_actor_session_id: Uuid::new_v4(),
        binding_provenance: None,
    }
}
fn term(value: &TypedTerm) -> serde_json::Value {
    serde_json::json!({"type":value.kind,"value":value.value,"datatype":value.datatype,"language":value.language})
}
fn rows(value: &KnowledgeUnitRevision) -> Vec<serde_json::Value> {
    let refs = RdfRefs {
        unit: value.unit_iri.clone(),
        revision: value.revision_iri.clone(),
        source: value.source_iri.clone(),
        event: value.publication_event_iri.clone(),
    };
    build_revision(refs,value.publication_operation,&value.constraint,None,&value.source_sha256,&value.publication_reason,&value.publication_authority_basis,value.publication_actor_principal_id,value.publication_actor_session_id).unwrap().triples.into_iter().map(|v|serde_json::json!({"subject":term(&v.subject),"predicate":term(&v.predicate),"object":term(&v.object)})).collect()
}

#[test]
fn exact_read_rejects_semantic_or_term_type_corruption() {
    let value = value();
    let original = rows(&value);
    assert_eq!(validate_rows(&original, &value), Ok(()));
    for (predicate, field, replacement) in [
        ("condition", "value", serde_json::json!("changed")),
        ("exception", "value", serde_json::json!("changed")),
        ("modality", "type", serde_json::json!("literal")),
        ("target", "type", serde_json::json!("literal")),
    ] {
        let mut altered = original.clone();
        let row = altered
            .iter_mut()
            .find(|row| row["predicate"]["value"] == format!("{DK}{predicate}"))
            .unwrap();
        row["object"][field] = replacement;
        assert_eq!(
            validate_rows(&altered, &value),
            Err(Error::InternalInvariant)
        );
    }
}

#[tokio::test]
async fn exact_read_rejects_native_graph_tamper() {
    if std::env::var("TECT_TEST_DURABLE_KNOWLEDGE").as_deref() != Ok("1") {
        return;
    }
    let pool = sqlx::PgPool::connect(
        &std::env::var("TECT_TEST_ADMIN_URL").expect("dedicated DK admin URL required"),
    )
    .await
    .unwrap();
    let tenant: Uuid = sqlx::query_scalar(
        "SELECT tenant_id FROM workspace_knowledge_state WHERE capability_ready ORDER BY activated_at DESC LIMIT 1",
    )
    .fetch_one(&pool)
    .await
    .expect("native workspace fixture required");
    let workspace = Uuid::new_v4();
    let value = value_in(tenant, workspace);
    let document = build_revision(
        RdfRefs {
            unit: value.unit_iri.clone(),
            revision: value.revision_iri.clone(),
            source: value.source_iri.clone(),
            event: value.publication_event_iri.clone(),
        },
        value.publication_operation,
        &value.constraint,
        None,
        &value.source_sha256,
        &value.publication_reason,
        &value.publication_authority_basis,
        value.publication_actor_principal_id,
        value.publication_actor_session_id,
    )
    .unwrap();
    let mut tx = pool.begin().await.unwrap();
    sqlx::query("SELECT pg_catalog.set_config('tect.tenant_id',$1,true)")
        .bind(tenant.to_string())
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("INSERT INTO workspaces(id,tenant_id,key) VALUES($1,$2,$3)")
        .bind(workspace)
        .bind(tenant)
        .bind(format!("rdf-tamper-{}", workspace.simple()))
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("INSERT INTO workspace_knowledge_state(tenant_id,workspace_id,capability_ready,pgrdf_version,activated_at) VALUES($1,$2,true,'0.6.34',pg_catalog.clock_timestamp())")
        .bind(tenant)
        .bind(workspace)
        .execute(&mut *tx)
        .await
        .unwrap();
    native_publish(
        &mut tx,
        tenant,
        workspace,
        value.publication_event_id,
        value.publication_operation,
        &document.payload,
        &document.stable_payload,
    )
    .await
    .unwrap();
    let graph = format!("urn:tect:dk:workspace:{tenant}:{workspace}");
    let graph_id: i64 = sqlx::query_scalar("SELECT pgrdf.graph_id($1)")
        .bind(&graph)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    let tamper = format!(
        "<{}> <{DK}condition> \"tampered-native-value\" .",
        value.revision_iri
    );
    sqlx::query("SELECT pgrdf.parse_turtle($1,$2)")
        .bind(tamper)
        .bind(graph_id)
        .execute(&mut *tx)
        .await
        .unwrap();
    let rows = native_rows(
        &mut tx,
        tenant,
        workspace,
        value.unit_id,
        value.revision,
        value.publication_event_id,
    )
    .await
    .unwrap();
    let result = validate_rows(&rows, &value);
    sqlx::query("SELECT pgrdf.drop_graph($1,true)")
        .bind(graph_id)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.rollback().await.unwrap();
    pool.close().await;
    assert_eq!(result, Err(Error::InternalInvariant));
}
