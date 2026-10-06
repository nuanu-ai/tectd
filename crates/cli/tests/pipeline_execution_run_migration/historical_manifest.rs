//! Synthetic historical reconstruction from verified public publication material.
//! This fixture does not claim a past capture or review took place.
use super::*;
use knowledge_lifecycle_support::CommittedKnowledge;
use sha2::{Digest, Sha256};
use sqlx::PgConnection;
use tect_domain::*;

#[path = "historical_manifest/definition_shape.rs"]
mod definition_shape;
#[path = "historical_manifest/publication.rs"]
mod publication;

pub(super) struct Retained {
    id: Uuid,
    row: Value,
}
fn sha(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
fn hash<T: serde::Serialize + ?Sized>(value: &T) -> String {
    sha(&serde_json::to_vec(value).unwrap())
}
fn id(value: &Value) -> Uuid {
    Uuid::parse_str(value.as_str().unwrap()).unwrap()
}

async fn facts(c: &mut PgConnection, run: Uuid, unit: Uuid, change: Uuid) -> Value {
    let run_row: Value =
        sqlx::query_scalar("SELECT to_jsonb(r) FROM slice_pipeline_runs r WHERE id=$1 FOR UPDATE")
            .bind(run)
            .fetch_one(&mut *c)
            .await
            .unwrap();
    let tenant = id(&run_row["tenant_id"]);
    let workspace = id(&run_row["workspace_id"]);
    let head: Value = sqlx::query_scalar("SELECT to_jsonb(h) FROM knowledge_unit_heads h WHERE tenant_id=$1 AND workspace_id=$2 AND unit_id=$3 FOR UPDATE")
        .bind(tenant).bind(workspace).bind(unit).fetch_one(&mut *c).await.unwrap();
    let revision: Value = sqlx::query_scalar("SELECT to_jsonb(r) FROM knowledge_revisions r WHERE tenant_id=$1 AND workspace_id=$2 AND unit_id=$3 AND revision=$4")
        .bind(tenant).bind(workspace).bind(unit).bind(head["accepted_revision"].as_i64().unwrap()).fetch_one(&mut *c).await.unwrap();
    let state: Value = sqlx::query_scalar("SELECT to_jsonb(s) FROM workspace_knowledge_state s WHERE tenant_id=$1 AND workspace_id=$2 FOR UPDATE")
        .bind(tenant).bind(workspace).fetch_one(&mut *c).await.unwrap();
    let bindings: Vec<Value> = sqlx::query_scalar("SELECT to_jsonb(b) FROM knowledge_bindings b JOIN knowledge_unit_heads h ON h.tenant_id=b.tenant_id AND h.workspace_id=b.workspace_id AND h.unit_id=b.unit_id JOIN native_scopes ns ON ns.tenant_id=b.tenant_id AND ns.workspace_id=b.workspace_id AND ns.id=$3 JOIN scope_candidate_sets sc ON sc.tenant_id=ns.tenant_id AND sc.workspace_id=ns.workspace_id AND sc.id=ns.source_candidate_set_id WHERE b.tenant_id=$1 AND b.workspace_id=$2 AND b.revision=h.accepted_revision AND (b.binding_kind='workspace' OR (b.binding_kind='program' AND b.program_id=sc.program_id) OR (b.binding_kind='scope' AND b.scope_id=$3) OR (b.binding_kind='slice' AND b.scope_id=$3 AND b.slice_id=$4) OR (b.binding_kind='slice_phase' AND b.scope_id=$3 AND b.slice_id=$4 AND b.phase_id=$5)) ORDER BY b.id")
        .bind(tenant).bind(workspace).bind(id(&run_row["scope_id"])).bind(id(&run_row["slice_id"]))
        .bind(run_row["current_phase_id"].as_str().unwrap()).fetch_all(&mut *c).await.unwrap();
    let publication: Value = sqlx::query_scalar("SELECT to_jsonb(e) FROM knowledge_publication_events e WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3")
        .bind(tenant).bind(workspace).bind(id(&revision["publication_event_id"])).fetch_one(&mut *c).await.unwrap();
    let origin: Value = sqlx::query_scalar("SELECT to_jsonb(c) FROM knowledge_lifecycle_changes c WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3 FOR UPDATE")
        .bind(tenant).bind(workspace).bind(change).fetch_one(&mut *c).await.unwrap();
    let validations: i64 = sqlx::query_scalar("SELECT count(*) FROM knowledge_validation_events WHERE tenant_id=$1 AND workspace_id=$2 AND unit_id=$3")
        .bind(tenant).bind(workspace).bind(unit).fetch_one(&mut *c).await.unwrap();
    let maintenance: i64 = sqlx::query_scalar("SELECT count(*) FROM knowledge_maintenance_signals WHERE tenant_id=$1 AND workspace_id=$2 AND unit_id=$3")
        .bind(tenant).bind(workspace).bind(unit).fetch_one(&mut *c).await.unwrap();
    json!({"run":run_row,"head":head,"revision":revision,"state":state,
        "bindings":bindings,"validations":validations,"maintenance":maintenance,"publication":publication,"origin":origin})
}

pub(super) async fn reconstruct(
    pool: &PgPool,
    client: &mut Mcp,
    old: &ResolvedPipeline,
    committed: &CommittedKnowledge,
) -> Retained {
    let receipt: KnowledgePublisherReceipt =
        serde_json::from_value(committed.receipt.clone()).unwrap();
    let exact: KnowledgeDocumentRevision =
        serde_json::from_value(committed.exact["document"].clone()).unwrap();
    assert_eq!(receipt.applied_operations.len(), 1);
    let operation = &receipt.applied_operations[0];
    assert_eq!(operation.unit_id, exact.unit_id);
    assert_eq!(operation.revision, Some(exact.revision));
    assert_eq!(operation.rdf_digest, exact.rdf_digest);
    assert_eq!(operation.unit_iri, exact.unit_iri);
    assert_eq!(operation.revision_iri.as_ref(), Some(&exact.revision_iri));
    assert_eq!(operation.operation, KnowledgeLifecycleOperation::Create);
    let origin = publication::origin(client, &receipt).await;
    assert_eq!(origin.operations.len(), 1);
    assert_eq!(origin.operations[0].unit_id, exact.unit_id);
    assert_eq!(origin.operations[0].operation_id, operation.operation_id);
    assert_eq!(
        origin.operations[0].operation,
        KnowledgeLifecycleOperation::Create
    );
    assert!(origin.operations[0].expected_revision.is_none());
    assert_eq!(origin.sources, exact.document.sources);
    assert_eq!(origin.source_pins.len(), origin.sources.len());
    assert_eq!(
        origin.source_revision, 0,
        "fresh unamended Create source set"
    );
    let run = id(&old.run()["id"]);
    let retained_before = historical_seed::rows(pool, run).await;
    let mut connection = pool.acquire().await.unwrap();
    let before = facts(&mut connection, run, exact.unit_id, receipt.change_id).await;
    drop(connection);
    let row = &before["run"];
    assert_eq!(row["revision"], 2);
    assert_eq!(row["status"], "active");
    assert_eq!(row["current_phase_id"], old.run()["current_phase_id"]);
    assert_eq!(row["definition_digest"], old.run()["definition_digest"]);
    let definition = definition_shape::parse_and_compare(&row["definition"]);
    assert!(row["knowledge_manifest_id"].is_null() && row["knowledge_manifest_digest"].is_null());
    assert!(row["inquiry"].is_null());
    let head = &before["head"];
    let revision = &before["revision"];
    assert_eq!(head["accepted_revision"], json!(exact.revision));
    assert_eq!(head["lifecycle"], "active");
    assert_eq!(head["active"], true);
    assert_eq!(head["payload_erased"], false);
    assert_eq!(revision["payload_erased"], false);
    assert_eq!(head["contract_version"], "dk-2");
    assert_eq!(revision["contract_version"], "dk-2");
    assert_eq!(head["access_scope"], "workspace_members");
    assert_eq!(revision["access_scope"], head["access_scope"]);
    assert_eq!(revision["rdf_digest"], json!(exact.rdf_digest));
    assert_eq!(revision["publication_event_id"], json!(operation.event_id));
    assert_eq!(
        revision["document_payload"],
        serde_json::to_value(&exact.document).unwrap()
    );
    assert_eq!(before["publication"]["id"], json!(operation.event_id));
    publication::assert_identity(
        &before["publication"],
        receipt.change_id,
        operation.operation_id,
    );
    assert_eq!(before["publication"]["payload_erased"], false);
    assert_eq!(before["origin"]["payload_erased"], false);
    assert_eq!(
        before["origin"]["sources"],
        serde_json::to_value(&origin.sources).unwrap()
    );
    assert_eq!(
        before["origin"]["source_pins"],
        serde_json::to_value(&origin.source_pins).unwrap()
    );
    assert_eq!(
        before["origin"]["source_revision"],
        json!(origin.source_revision)
    );
    assert_eq!(
        serde_json::to_value(exact.lifecycle).unwrap(),
        head["lifecycle"]
    );
    assert_eq!(before["validations"], 0);
    assert_eq!(before["maintenance"], 0);
    assert!(
        exact.document.valid_from.is_none()
            && exact.document.valid_until.is_none()
            && exact.document.review_due_at.is_none()
    );
    assert!(exact.document.planning_briefs.is_empty());
    assert_eq!(
        before["state"]["generation"],
        json!(receipt.workspace_generation)
    );
    assert_eq!(before["state"]["capability_ready"], true);
    let bindings = before["bindings"].as_array().unwrap();
    assert_eq!(bindings.len(), 1, "no extra applicable DK1 or DK2 binding");
    let binding = &bindings[0];
    assert_eq!(binding["unit_id"], json!(exact.unit_id));
    assert_eq!(binding["revision"], json!(exact.revision));
    assert_eq!(binding["active"], true);
    assert_eq!(binding["contract_version"], "dk-2");
    assert_eq!(binding["binding_kind"], "slice_phase");
    assert!(binding["program_id"].is_null());
    for field in [
        "scope_id",
        "slice_id",
        "phase_id",
        "definition_kind",
        "definition_version",
        "definition_digest",
    ] {
        let source = if field == "phase_id" {
            &row["current_phase_id"]
        } else {
            &row[field]
        };
        assert_eq!(&binding[field], source);
    }
    assert_eq!(binding["purpose"], "required");
    assert_eq!(binding["version_resolution"], "current_accepted");
    assert!(binding["pinned_revision"].is_null());
    let source_pins = origin
        .sources
        .iter()
        .zip(&origin.source_pins)
        .enumerate()
        .map(|(index, (source, pin))| {
            let KnowledgeSourceRef::Snapshot { snapshot } = source else {
                panic!("only actual snapshot sources supported")
            };
            assert_eq!(pin.source_index, index as u32);
            assert_eq!(pin.digest, sha(snapshot.text.as_bytes()));
            assert_eq!(exact.source_digests[index], pin.digest);
            assert_eq!(pin.evidence_kind, snapshot.evidence_kind);
            assert_eq!(pin.observed_at, snapshot.observed_at);
            assert_eq!(pin.evidence_scope, snapshot.title);
            assert!(!pin.source_iri.is_empty());
            PipelineKnowledgeSourcePin {
                source_iri: pin.source_iri.clone(),
                digest: pin.digest.clone(),
                evidence_kind: pin.evidence_kind,
                observed_at: pin.observed_at.clone(),
                evidence_scope: pin.evidence_scope.clone(),
                title: snapshot.title.clone(),
                uri: snapshot.uri.clone(),
            }
        })
        .collect();
    assert_eq!(exact.source_digests.len(), origin.source_pins.len());
    let resource = PipelineKnowledgeResource {
        unit_id: exact.unit_id,
        revision: exact.revision,
        lifecycle: exact.lifecycle,
        access_scope: serde_json::from_value(head["access_scope"].clone()).unwrap(),
        rdf_digest: exact.rdf_digest.clone(),
        unit_iri: exact.unit_iri.clone(),
        revision_iri: exact.revision_iri.clone(),
        title: exact.document.title.clone(),
        canonical_text: exact.document.canonical_text.clone(),
        knowledge_kind: exact.document.knowledge_kind,
        epistemic_state: exact.document.epistemic_state,
        target_iris: exact.document.target_iris.clone(),
        profiles: exact.document.profiles.clone(),
        conditions: exact.document.conditions.clone(),
        exceptions: exact.document.exceptions.clone(),
        sections: exact.document.sections.clone(),
        inquiry_briefs: None,
        source_pins,
        latest_validation: None,
        binding: PipelineKnowledgeBindingPin {
            binding_iri: format!("urn:tect:dk:binding:{}", id(&binding["id"])),
            target: KnowledgeBindingTarget::SlicePhase {
                scope_id: id(&row["scope_id"]),
                slice_id: id(&row["slice_id"]),
                phase_id: row["current_phase_id"].as_str().unwrap().into(),
            },
            purpose: serde_json::from_value(binding["purpose"].clone()).unwrap(),
            version_resolution: KnowledgeBindingVersion::CurrentAccepted,
            definition_kind: Some(binding["definition_kind"].as_str().unwrap().into()),
            definition_version: Some(binding["definition_version"].as_str().unwrap().into()),
            definition_digest: Some(binding["definition_digest"].as_str().unwrap().into()),
        },
        why_included: "slice_phase_binding".into(),
    };
    assert_eq!(exact.document.bindings.len(), 1);
    assert_eq!(exact.document.bindings[0].target, resource.binding.target);
    assert_eq!(exact.document.bindings[0].purpose, resource.binding.purpose);
    assert_eq!(
        exact.document.bindings[0].version_resolution,
        resource.binding.version_resolution
    );
    assert_eq!(exact.document.access_scope, resource.access_scope);
    let phases = definition
        .phases
        .iter()
        .filter(|p| p.id == row["current_phase_id"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(phases.len(), 1);
    let phase = phases[0];
    let mut methods =
        phase
            .instructions
            .iter()
            .chain(&phase.skills)
            .chain(&phase.resources)
            .map(|v| KnowledgeContractRef {
                id: v.id.clone(),
                version: v.version.clone(),
                digest: v.digest.clone(),
                source_ref: v.origin_refs.first().cloned().unwrap_or_else(|| {
                    format!("embedded:{}:{}", definition.kind.as_str(), phase.id)
                }),
            })
            .collect::<Vec<_>>();
    methods.sort();
    methods.dedup();
    let empty: Vec<String> = Vec::new();
    let legacy_selected: Vec<PipelineKnowledgeItem> = Vec::new();
    let legacy_material = legacy_selected
        .iter()
        .map(|v| {
            (
                &v.unit_id,
                v.revision,
                &v.rdf_digest,
                &v.source_sha256,
                &v.statement,
                v.modality,
                &v.action,
                &v.target_iri,
                &v.conditions,
                &v.exceptions,
            )
        })
        .collect::<Vec<_>>();
    let legacy_semantic = hash(&(legacy_material, &empty));
    let mut manifest = PipelineKnowledgeResourceManifest {
        id: Uuid::new_v4(),
        digest: String::new(),
        semantic_digest: hash(&(
            &definition.version,
            &definition.digest,
            &methods,
            vec![resource.clone()],
            &empty,
            &empty,
        )),
        workspace_generation: receipt.workspace_generation,
        run_id: run,
        run_revision: 2,
        phase_id: phase.id.clone(),
        definition_version: definition.version.clone(),
        definition_digest: definition.digest.clone(),
        method_requirements: methods,
        inquiry: None,
        projection_policy: None,
        selected: vec![resource],
        unresolved_needs: Vec::new(),
        freshness_warnings: Vec::new(),
    };
    let outer = outer_hash(&manifest, &legacy_selected, &legacy_semantic);
    manifest.digest = outer;
    let mut tx = pool.begin().await.unwrap();
    assert_eq!(
        before,
        facts(&mut tx, run, exact.unit_id, receipt.change_id).await,
        "preimage pins unchanged before synthetic insert"
    );
    let tenant = id(&row["tenant_id"]);
    let workspace = id(&row["workspace_id"]);
    sqlx::query("INSERT INTO pipeline_knowledge_manifests(id,tenant_id,workspace_id,run_id,run_revision,phase_id,workspace_generation,digest,semantic_digest,selected,unresolved_needs,contract_version,definition_version,definition_digest,method_requirements,selected_resources,resource_unresolved_needs,freshness_warnings,resource_semantic_digest,resource_inquiry,resource_projection_policy) VALUES($1,$2,$3,$4,2,$5,$6,$7,$8,'[]','[]','dk-2',$9,$10,$11,$12,'[]','[]',$13,NULL,NULL)")
        .bind(manifest.id).bind(tenant).bind(workspace).bind(run).bind(&manifest.phase_id).bind(manifest.workspace_generation).bind(&manifest.digest).bind(&legacy_semantic)
        .bind(&manifest.definition_version).bind(&manifest.definition_digest).bind(serde_json::to_value(&manifest.method_requirements).unwrap())
        .bind(serde_json::to_value(&manifest.selected).unwrap()).bind(&manifest.semantic_digest).execute(&mut *tx).await.unwrap();
    let consumer = Uuid::new_v4();
    sqlx::query("INSERT INTO knowledge_owned_copies(id,tenant_id,workspace_id,unit_id,copy_kind,relation_name,row_id,row_revision) VALUES($1,$2,$3,$4,'pipeline_manifest','pipeline_knowledge_manifests',$5,$6)")
        .bind(Uuid::new_v4()).bind(tenant).bind(workspace).bind(exact.unit_id).bind(manifest.id).bind(exact.revision).execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO knowledge_maintenance_consumers(id,tenant_id,workspace_id,unit_id,unit_revision,consumer_ref,required,relation_name,row_id) VALUES($1,$2,$3,$4,$5,$6,true,'pipeline_knowledge_manifests',$7)")
        .bind(consumer).bind(tenant).bind(workspace).bind(exact.unit_id).bind(exact.revision).bind(format!("pipeline-manifest:{}",manifest.id)).bind(manifest.id).execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO knowledge_owned_copies(id,tenant_id,workspace_id,unit_id,copy_kind,relation_name,row_id,source_revision) VALUES($1,$2,$3,$4,'maintenance_consumer','knowledge_maintenance_consumers',$5,$6)")
        .bind(Uuid::new_v4()).bind(tenant).bind(workspace).bind(exact.unit_id).bind(consumer).bind(exact.revision).execute(&mut *tx).await.unwrap();
    let updated=sqlx::query("UPDATE slice_pipeline_runs SET knowledge_manifest_id=$2,knowledge_manifest_digest=$3 WHERE id=$1 AND revision=2 AND status='active' AND current_phase_id=$4 AND definition_digest=$5 AND knowledge_manifest_id IS NULL AND knowledge_manifest_digest IS NULL")
        .bind(run).bind(manifest.id).bind(&manifest.digest).bind(&manifest.phase_id).bind(&manifest.definition_digest).execute(&mut *tx).await.unwrap();
    assert_eq!(updated.rows_affected(), 1);
    tx.commit().await.unwrap();
    let after = historical_seed::rows(pool, run).await;
    for (key, value) in retained_before["run"].as_object().unwrap() {
        if key != "knowledge_manifest_id" && key != "knowledge_manifest_digest" {
            assert_eq!(&after["run"][key], value, "historical run {key} preserved");
        }
    }
    for key in ["attempts", "outputs", "bindings", "receipts", "successors"] {
        assert_eq!(after[key], retained_before[key]);
    }
    let mut connection = pool.acquire().await.unwrap();
    let after_facts = facts(&mut connection, run, exact.unit_id, receipt.change_id).await;
    drop(connection);
    for key in [
        "head",
        "revision",
        "state",
        "bindings",
        "validations",
        "maintenance",
        "publication",
        "origin",
    ] {
        assert_eq!(after_facts[key], before[key]);
    }
    let stored: Value =
        sqlx::query_scalar("SELECT to_jsonb(m) FROM pipeline_knowledge_manifests m WHERE id=$1")
            .bind(manifest.id)
            .fetch_one(pool)
            .await
            .unwrap();
    roundtrip(&stored, &manifest, &legacy_selected, &legacy_semantic);
    let raw = route(
        client,
        "query",
        "slice.pipeline.context",
        json!({"run_id":run}),
    )
    .await;
    let actual = resolve_pipeline(client, raw)
        .await
        .expect("read synthetic historical manifest through actual pipeline destinations");
    let public: PipelineKnowledgeResourceManifest =
        serde_json::from_value(actual.details_data()["knowledge_resources"].clone()).unwrap();
    assert_eq!(public, manifest);
    Retained {
        id: manifest.id,
        row: stored,
    }
}

fn outer_hash(
    m: &PipelineKnowledgeResourceManifest,
    selected: &[PipelineKnowledgeItem],
    semantic: &str,
) -> String {
    let empty: Vec<String> = Vec::new();
    hash(&(
        m.id,
        m.workspace_generation,
        m.run_id,
        m.run_revision,
        m.phase_id.as_str(),
        selected,
        &empty,
        semantic,
        &m.semantic_digest,
        &m.definition_version,
        &m.definition_digest,
        &m.method_requirements,
        &m.selected,
        &m.unresolved_needs,
        &m.freshness_warnings,
    ))
}
fn roundtrip(
    row: &Value,
    m: &PipelineKnowledgeResourceManifest,
    selected: &[PipelineKnowledgeItem],
    semantic: &str,
) {
    let resources: Vec<PipelineKnowledgeResource> =
        serde_json::from_value(row["selected_resources"].clone()).unwrap();
    let methods: Vec<KnowledgeContractRef> =
        serde_json::from_value(row["method_requirements"].clone()).unwrap();
    let stored = PipelineKnowledgeResourceManifest {
        id: id(&row["id"]),
        digest: row["digest"].as_str().unwrap().into(),
        semantic_digest: row["resource_semantic_digest"].as_str().unwrap().into(),
        workspace_generation: row["workspace_generation"].as_i64().unwrap(),
        run_id: id(&row["run_id"]),
        run_revision: row["run_revision"].as_i64().unwrap(),
        phase_id: row["phase_id"].as_str().unwrap().into(),
        definition_version: row["definition_version"].as_str().unwrap().into(),
        definition_digest: row["definition_digest"].as_str().unwrap().into(),
        method_requirements: methods.clone(),
        inquiry: None,
        projection_policy: None,
        selected: resources.clone(),
        unresolved_needs: serde_json::from_value(row["resource_unresolved_needs"].clone()).unwrap(),
        freshness_warnings: serde_json::from_value(row["freshness_warnings"].clone()).unwrap(),
    };
    assert_eq!(&stored, m, "typed SQL manifest roundtrip");
    assert_eq!(resources, m.selected);
    assert_eq!(methods, m.method_requirements);
    assert_eq!(
        row["resource_semantic_digest"],
        json!(hash(&(
            &m.definition_version,
            &m.definition_digest,
            &methods,
            &resources,
            Vec::<String>::new(),
            Vec::<String>::new()
        )))
    );
    assert_eq!(
        row["digest"],
        json!(outer_hash(&stored, selected, semantic))
    );
    assert_eq!(row["semantic_digest"], json!(semantic));
    assert_eq!(row["selected"], json!([]));
    for field in [
        "unresolved_needs",
        "resource_unresolved_needs",
        "freshness_warnings",
    ] {
        assert_eq!(row[field], json!([]));
    }
    assert!(row["resource_inquiry"].is_null() && row["resource_projection_policy"].is_null());
}
pub(super) async fn assert_unchanged(pool: &PgPool, retained: &Retained) {
    let row: Value =
        sqlx::query_scalar("SELECT to_jsonb(m) FROM pipeline_knowledge_manifests m WHERE id=$1")
            .bind(retained.id)
            .fetch_one(pool)
            .await
            .unwrap();
    assert_eq!(
        row, retained.row,
        "synthetic historical manifest is immutable across retirement"
    );
}
