use super::*;
fn value() -> KnowledgeUnitRevision {
    let tenant = Uuid::new_v4();
    let workspace = Uuid::new_v4();
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
    let (tenant, workspace, unit, revision, revision_iri): (Uuid, Uuid, Uuid, i64, String) =
        sqlx::query_as("SELECT tenant_id,workspace_id,unit_id,revision,revision_iri FROM knowledge_revisions ORDER BY created_at DESC LIMIT 1")
            .fetch_one(&pool).await.expect("native lifecycle fixture required");
    let graph = format!("urn:tect:dk:workspace:{tenant}:{workspace}");
    let graph_id: i64 = sqlx::query_scalar("SELECT pgrdf.graph_id($1)")
        .bind(&graph)
        .fetch_one(&pool)
        .await
        .unwrap();
    let tamper = format!("<{revision_iri}> <{DK}condition> \"tampered-native-value\" .");
    sqlx::query("SELECT pgrdf.parse_turtle($1,$2)")
        .bind(tamper)
        .bind(graph_id)
        .execute(&pool)
        .await
        .unwrap();
    let mut tx = pool.begin().await.unwrap();
    sqlx::query("SELECT pg_catalog.set_config('tect.tenant_id',$1,true)")
        .bind(tenant.to_string())
        .execute(&mut *tx)
        .await
        .unwrap();
    let result = super::super::context::load_revision(
        &mut tx,
        tenant,
        workspace,
        unit,
        Some(revision),
        true,
    )
    .await;
    assert_eq!(result, Err(Error::InternalInvariant));
    tx.rollback().await.unwrap();
    pool.close().await;
}
