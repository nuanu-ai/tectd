// Synthetic fixture mechanics only; no genuine Owner/provider authority proof.
use ring::signature::{Ed25519KeyPair, KeyPair};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

#[derive(Clone)]
struct PositiveDefinitions(Vec<PipelineDefinitionSnapshot>);

impl PositiveDefinitions {
    fn new() -> Self {
        let published = tect_host::StaticPipelineRecommendationDefinitions;
        let values = [
            PipelineKind::DebugRootCause,
            PipelineKind::FullDesignToExecution,
        ]
        .into_iter()
        .map(|kind| {
            let value = published.definition("4", kind).unwrap().unwrap();
            value.validate().unwrap();
            value
        })
        .collect();
        Self(values)
    }
}

impl PipelineRecommendationDefinitionProvider for PositiveDefinitions {
    fn definition(
        &self,
        revision: &str,
        kind: PipelineKind,
    ) -> Result<Option<PipelineDefinitionSnapshot>> {
        Ok((revision == "4")
            .then(|| self.0.iter().find(|value| value.kind == kind).cloned())
            .flatten())
    }
}

struct PositiveGuidance(PositiveDefinitions);
impl NativePlanningGuidance for PositiveGuidance {
    fn snapshot(
        &self,
        basis: &ScopeOpenBasis,
        inputs: &[SlicePlanningInput],
        results: &[SliceResult],
    ) -> Result<SlicePlanningSnapshotMaterial> {
        let mut material = native::Guidance.snapshot(basis, inputs, results)?;
        let template = material.catalogue.entries[0].clone();
        material.catalogue.entries = PipelineKind::CURRENT_SLICE_RUN_KINDS
            .into_iter()
            .chain([PipelineKind::PromoteToDurableKnowledge])
            .map(|kind| {
                let mut entry = template.clone();
                entry.kind = kind;
                if let Some(definition) = self.0.0.iter().find(|value| value.kind == kind) {
                    entry.default_delivery_mode = Some(definition.default_mode);
                    entry.allowed_delivery_modes = definition.allowed_modes.clone();
                }
                if kind == PipelineKind::PromoteToDurableKnowledge {
                    entry.execution_owner = PipelineExecutionOwner::KnowledgeChange;
                }
                entry
            })
            .collect();
        material.catalogue.revision = "4".into();
        material.catalogue.digest = format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(&material.catalogue.entries).unwrap())
        );
        material.catalogue.validate()?;
        Ok(material)
    }
}

fn positive_policy(
    source: &MatrixTaskSource,
    cards: &EngineeringMatrixComposition,
    definitions: &PositiveDefinitions,
) -> PipelineCompatibilityPolicy {
    let MatrixFact::Known { value: mode, .. } = &source.revision.input.mode else {
        panic!("known fixture Matrix mode required");
    };
    let value = PipelineCompatibilityPolicy {
        version: PIPELINE_COMPATIBILITY_POLICY_VERSION.into(),
        task_id: source.revision.task_id.to_string(),
        task_revision: source.revision.revision.to_string(),
        catalogue_revision: "4".into(),
        rules: definitions
            .0
            .iter()
            .map(|definition| {
                let plan = PipelineVerificationPlan::from_definition(definition).unwrap();
                let obligation = plan
                    .obligations
                    .iter()
                    .find(|value| {
                        !value.required_fields.is_empty()
                            || !value.required_artifacts.is_empty()
                            || !value.validator_contracts.is_empty()
                            || !value.output_constraints.is_empty()
                    })
                    .expect("actual required-phase verification duty");
                PipelineCompatibilityRule {
                    kind: definition.kind,
                    matrix_input_digest: matrix_input_digest(&source.revision.input).unwrap(),
                    allowed_modes: vec![*mode],
                    selected_candidate_ids: vec!["a".into()],
                    card_coverage: cards
                        .mandatory_cards
                        .iter()
                        .map(|card| PipelineCardCoverage {
                            card_id: card.id.to_string(),
                            phase_id: obligation.phase_id.clone(),
                            obligation_digest: pipeline_obligation_digest(obligation).unwrap(),
                        })
                        .collect(),
                }
            })
            .collect(),
    };
    value.validate_host_snapshot("4").unwrap();
    value
}

fn signed_test_budget(
    workspace: Uuid,
    owner: Uuid,
) -> (AdvisoryBudgetPolicy, crate::BudgetOwnerKeys) {
    // Same test-only signing mechanics as Scope's private fixture. No real Owner key.
    let now = i64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis(),
    )
    .unwrap();
    let (from, until, id) = (now - 60_000, now + 3_600_000, Uuid::new_v4());
    let ceilings = AdvisoryBudgetCeilings {
        provider_calls: 2,
        input_tokens: 1024,
        output_tokens: 1024,
        request_utf8_bytes: tect_host::jev_pipeline_recommendation::MAX_REQUEST_BYTES as i64,
        elapsed_monotonic_ms: 10_000,
        retry_dispatches: 1,
    };
    let digest = AdvisoryBudgetPolicy::digest_for(id, 1, from, until, ceilings);
    let unsigned = AdvisoryBudgetPolicy::new(
        id,
        1,
        digest.clone(),
        from,
        until,
        ceilings,
        owner,
        "0".repeat(128),
    )
    .unwrap();
    let pair = Ed25519KeyPair::from_seed_unchecked(&[7; 32]).unwrap();
    let hex = |bytes: &[u8]| -> String { bytes.iter().map(|byte| format!("{byte:02x}")).collect() };
    let signature = pair.sign(&unsigned.approval_signing_message(workspace).unwrap());
    let signed = AdvisoryBudgetPolicy::new(
        id,
        1,
        digest,
        from,
        until,
        ceilings,
        owner,
        hex(signature.as_ref()),
    )
    .unwrap();
    let keys = crate::BudgetOwnerKeys::from_json(
        &json!([{
            "workspace_id": workspace, "owner_id": owner,
            "public_key_hex": hex(pair.public_key().as_ref()),
        }])
        .to_string(),
    )
    .unwrap();
    (signed, keys)
}

struct CapturedHttp {
    request: Vec<u8>,
    response: Vec<u8>,
    completed_requests: usize,
    accepted_connections: usize,
}

fn pipeline_response(request: &Value) -> Vec<u8> {
    let options = request["state"]["manifest"]["options"].as_array().unwrap();
    assert_eq!(options.len(), 2);
    let levels = request["questions"]["score_v1_0"]["criteria"]
        .as_array()
        .unwrap();
    assert_eq!(levels.len(), 10);
    let legend = levels
        .iter()
        .enumerate()
        .map(|(index, value)| (index.to_string(), value.clone()))
        .collect::<serde_json::Map<_, _>>();
    let mut answers = serde_json::Map::new();
    let mut choices = serde_json::Map::new();
    for (index, option) in options.iter().enumerate() {
        let id = option["id"].as_str().unwrap();
        let level = 9 - index;
        let probabilities = (0..10)
            .map(|value| {
                (
                    value.to_string(),
                    json!(if value == level { 1.0 } else { 0.0 }),
                )
            })
            .collect::<serde_json::Map<_, _>>();
        answers.insert(
            format!("score_v1_{index}"),
            json!({"type":"score",
            "score":level,"legend":legend,"probabilities":probabilities,"confidence":0.91}),
        );
        choices.insert(id.to_owned(), json!(if index == 0 { 0.8 } else { 0.0 }));
    }
    choices.insert("ABSTAIN".into(), json!(0.2));
    answers.insert(
        "choice_v1".into(),
        json!({"type":"choice",
        "choice":options[0]["id"],"probabilities":choices,"confidence":0.8}),
    );
    serde_json::to_vec(&json!({"model":request["model"],"answers":answers,
        "usage":{"input_tokens":20,"output_tokens":30}}))
    .unwrap()
}

fn serve_once(
    listener: TcpListener,
    done: tokio::sync::oneshot::Receiver<()>,
    captured: tokio::sync::oneshot::Sender<(Vec<u8>, Vec<u8>)>,
    release: tokio::sync::oneshot::Receiver<()>,
) -> tokio::task::JoinHandle<CapturedHttp> {
    tokio::spawn(async move {
        tokio::time::timeout(std::time::Duration::from_secs(15), async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            let mut buffer = [0; 4096];
            let header_end = loop {
                let n = socket.read(&mut buffer).await.unwrap();
                assert!(n > 0);
                assert!(request.len() + n <= 524288 + 8192);
                request.extend_from_slice(&buffer[..n]);
                if let Some(index) = request.windows(4).position(|value| value == b"\r\n\r\n") {
                    break index + 4;
                }
                assert!(request.len() <= 8192);
            };
            let headers = std::str::from_utf8(&request[..header_end]).unwrap();
            assert!(headers.starts_with("POST /v1/systemone HTTP/1.1\r\n"));
            assert!(headers.to_ascii_lowercase().contains("authorization: bearer synthetic-pipeline-only\r\n"));
            let length: usize = headers.lines().find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("content-length").then(|| value.trim().parse().unwrap())
            }).unwrap();
            assert!(length <= 524288);
            while request.len() < header_end + length {
                let n = socket.read(&mut buffer).await.unwrap();
                assert!(n > 0 && request.len() + n <= header_end + length);
                request.extend_from_slice(&buffer[..n]);
            }
            assert_eq!(request.len(), header_end + length);
            let body = request[header_end..].to_vec();
            let response = pipeline_response(&serde_json::from_slice(&body).unwrap());
            assert!(response.len() <= 65536);
            captured.send((body.clone(), response.clone())).expect("guard capture receiver");
            release.await.expect("guard response release");
            socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", response.len()).as_bytes()).await.unwrap();
            socket.write_all(&response).await.unwrap();
            socket.shutdown().await.unwrap();
            drop(socket);
            let second = tokio::select! { biased;
                accepted = listener.accept() => accepted.is_ok(),
                _ = done => false,
            };
            CapturedHttp { request: body, response, completed_requests: 1,
                accepted_connections: 1 + usize::from(second) }
        }).await.expect("bounded fake HTTP lifetime")
    })
}

async fn rows(
    pool: &PgPool,
    tenant: Uuid,
    workspace: Uuid,
    opportunity: Uuid,
) -> std::result::Result<Value, sqlx::Error> {
    let mut tx = pool.begin().await?;
    sqlx::query("SELECT pg_catalog.set_config('tect.tenant_id',$1,true)")
        .bind(tenant.to_string())
        .execute(&mut *tx)
        .await?;
    let value = rows_in(&mut tx, tenant, workspace, opportunity).await?;
    tx.commit().await?;
    Ok(value)
}

async fn rows_in(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    opportunity: Uuid,
) -> std::result::Result<Value, sqlx::Error> {
    let mut value = serde_json::Map::new();
    for table in [
        "advisory_opportunity",
        "advisory_dispatch",
        "advisory_provider_observations",
        "advisory_budget_reservations",
        "advisory_budget_consumptions",
    ] {
        let key = if table == "advisory_opportunity" {
            "id"
        } else {
            "opportunity_id"
        };
        let sql = format!(
            "SELECT COALESCE(jsonb_agg(to_jsonb(r) ORDER BY to_jsonb(r)::text),'[]'::jsonb) FROM {table} r WHERE tenant_id=$1 AND workspace_id=$2 AND {key}=$3"
        );
        let result: Value = sqlx::query_scalar(&sql)
            .bind(tenant)
            .bind(workspace)
            .bind(opportunity)
            .fetch_one(&mut **tx)
            .await?;
        value.insert(table.into(), result);
    }
    Ok(Value::Object(value))
}
