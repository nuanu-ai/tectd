use super::super::*;

// Only this disposable fixture owns the deterministic private seed.
pub(in super::super) fn signed_scope_budget_fixture(
    workspace: Uuid,
    owner: Uuid,
) -> (tect_domain::AdvisoryBudgetPolicy, crate::BudgetOwnerKeys) {
    use ring::signature::{Ed25519KeyPair, KeyPair};
    use tect_domain::{AdvisoryBudgetCeilings, AdvisoryBudgetPolicy};

    let now = i64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis(),
    )
    .unwrap();
    let from = now.checked_sub(60_000).unwrap();
    let until = now.checked_add(3_600_000).unwrap();
    let id = Uuid::new_v4();
    let version = 1;
    let ceilings = AdvisoryBudgetCeilings {
        provider_calls: 2,
        input_tokens: 1_024,
        output_tokens: 1_024,
        request_utf8_bytes: 65_536,
        elapsed_monotonic_ms: 10_000,
        retry_dispatches: 1,
    };
    let digest = AdvisoryBudgetPolicy::digest_for(id, version, from, until, ceilings);
    let unsigned = AdvisoryBudgetPolicy::new(
        id,
        version,
        digest.clone(),
        from,
        until,
        ceilings,
        owner,
        "0".repeat(128),
    )
    .unwrap();
    let keypair = Ed25519KeyPair::from_seed_unchecked(&[7u8; 32]).unwrap();
    let signature = keypair.sign(&unsigned.approval_signing_message(workspace).unwrap());
    let hex = |bytes: &[u8]| -> String { bytes.iter().map(|byte| format!("{byte:02x}")).collect() };
    let signed = AdvisoryBudgetPolicy::new(
        id,
        version,
        digest,
        from,
        until,
        ceilings,
        owner,
        hex(signature.as_ref()),
    )
    .unwrap();
    let keys = crate::BudgetOwnerKeys::from_json(
        &serde_json::json!([{
            "workspace_id": workspace,
            "owner_id": owner,
            "public_key_hex": hex(keypair.public_key().as_ref()),
        }])
        .to_string(),
    )
    .unwrap();
    (signed, keys)
}

pub(in super::super) async fn fake_jev_once() -> (
    reqwest::Url,
    tokio::sync::oneshot::Sender<()>,
    tokio::task::JoinHandle<(Vec<u8>, bool)>,
) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = reqwest::Url::parse(&format!(
        "http://{}/v1/systemone",
        listener.local_addr().unwrap()
    ))
    .unwrap();
    let (done_sender, done_receiver) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut request = Vec::new();
        let mut buffer = [0_u8; 4096];
        let header_end = loop {
            let read = socket.read(&mut buffer).await.unwrap();
            assert!(read > 0);
            request.extend_from_slice(&buffer[..read]);
            if let Some(position) = request.windows(4).position(|part| part == b"\r\n\r\n") {
                break position + 4;
            }
        };
        let headers = String::from_utf8_lossy(&request[..header_end]);
        let length: usize = headers
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse().unwrap())
            })
            .unwrap();
        while request.len() < header_end + length {
            let read = socket.read(&mut buffer).await.unwrap();
            assert!(read > 0);
            request.extend_from_slice(&buffer[..read]);
        }
        let body = request[header_end..].to_vec();
        let parsed: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let response = serde_json::to_vec(
            &fixture_v3_response(&parsed).expect("invalid current-v3 fixture request"),
        )
        .unwrap();
        let head = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            response.len()
        );
        socket.write_all(head.as_bytes()).await.unwrap();
        socket.write_all(&response).await.unwrap();
        let second_call = tokio::select! {
            biased;
            accepted = listener.accept() => accepted.is_ok(),
            _ = done_receiver => false,
        };
        (body, second_call)
    });
    (endpoint, done_sender, server)
}

fn fixture_v3_response(
    parsed: &serde_json::Value,
) -> std::result::Result<serde_json::Value, &'static str> {
    if parsed["model"].as_str() != Some("jev") {
        return Err("unsupported fixture model");
    }
    let alternatives = parsed["state"]["request"]["alternatives"]
        .as_array()
        .filter(|values| !values.is_empty())
        .ok_or("missing or empty alternatives")?;
    let mut sorted = std::collections::BTreeMap::new();
    for alternative in alternatives {
        let id = alternative["id"]
            .as_str()
            .filter(|id| !id.is_empty())
            .ok_or("invalid alternative ID")?;
        if sorted.insert(id, alternative).is_some() {
            return Err("duplicate alternative ID");
        }
    }
    let tokens = parsed["state"]["candidate_tokens"]
        .as_object()
        .ok_or("missing candidate token mapping")?;
    if tokens.len() != sorted.len() {
        return Err("incomplete or extra candidate token mapping");
    }
    let mut answers = serde_json::Map::new();
    let mut probabilities = serde_json::Map::new();
    for (index, (id, alternative)) in sorted.into_iter().enumerate() {
        let token = format!("C{index}");
        if tokens.get(&token) != Some(alternative) {
            return Err("candidate token binding mismatch");
        }
        probabilities.insert(token, serde_json::json!(if index == 0 { 0.8 } else { 0.0 }));
        answers.insert(
            format!("score_{id}"),
            serde_json::json!({
                "type":"score", "score":2.4, "confidence":0.7,
                "legend":{"0":"conflict","1":"weak_fit","2":"fit","3":"strong_fit"},
                "probabilities":{"0":0.05,"1":0.1,"2":0.55,"3":0.3}
            }),
        );
    }
    probabilities.insert("ABSTAIN".into(), serde_json::json!(0.2));
    answers.insert(
        "choice_v3".into(),
        serde_json::json!({
            "type":"choice", "choice":"C0", "confidence":0.8,
            "probabilities":probabilities
        }),
    );
    Ok(serde_json::json!({
        "model":"jev", "answers":answers,
        "usage":{"input_tokens":11,"output_tokens":5}
    }))
}

#[test]
fn fixture_v3_single_and_reordered_numeric_tokens() {
    for count in [1, 12] {
        let mut alternatives = (0..count).map(|index| serde_json::json!({
            "id":format!("opaque-{index:02}"), "material_digest":format!("material-{index}"),
            "kind":"cohesive", "covered_obligation_ids":["obligation.one"]
        })).collect::<Vec<_>>();
        let tokens = alternatives
            .iter()
            .enumerate()
            .map(|(index, alternative)| (format!("C{index}"), alternative.clone()))
            .collect::<serde_json::Map<_, _>>();
        alternatives.reverse();
        let parsed = serde_json::json!({"model":"jev", "state":{
            "request":{"alternatives":alternatives}, "candidate_tokens":tokens
        }});
        let response = fixture_v3_response(&parsed).unwrap();
        assert_eq!(response["model"], parsed["model"]);
        assert_eq!(
            response["usage"],
            serde_json::json!({"input_tokens":11,"output_tokens":5})
        );
        let answers = response["answers"].as_object().unwrap();
        let mut expected = parsed["state"]["request"]["alternatives"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| format!("score_{}", value["id"].as_str().unwrap()))
            .collect::<std::collections::BTreeSet<_>>();
        expected.insert("choice_v3".into());
        assert_eq!(
            answers
                .keys()
                .cloned()
                .collect::<std::collections::BTreeSet<_>>(),
            expected
        );
        assert_eq!(answers["choice_v3"]["choice"], "C0");
        let probabilities = answers["choice_v3"]["probabilities"].as_object().unwrap();
        assert_eq!(probabilities.len(), count + 1);
        for index in 0..count {
            assert_eq!(
                probabilities[&format!("C{index}")],
                serde_json::json!(if index == 0 { 0.8 } else { 0.0 })
            );
        }
        assert_eq!(probabilities["ABSTAIN"], 0.2);
        assert!(
            (probabilities
                .values()
                .map(|value| value.as_f64().unwrap())
                .sum::<f64>()
                - 1.0)
                .abs()
                < 1e-6
        );
    }
}

#[test]
fn fixture_v3_rejects_malformed_or_unsupported_mapping() {
    let alternative = serde_json::json!({"id":"opaque-z", "material_digest":"original"});
    let valid = serde_json::json!({"model":"jev", "state":{
        "request":{"alternatives":[alternative.clone()]}, "candidate_tokens":{"C0":alternative.clone()}
    }});
    assert!(fixture_v3_response(&valid).is_ok());
    for case in 0..8 {
        let mut parsed = valid.clone();
        match case {
            0 => parsed["state"]["request"]["alternatives"] = serde_json::json!([]),
            1 => {
                parsed["state"]["request"]["alternatives"] =
                    serde_json::json!([alternative.clone(), alternative.clone()])
            }
            2 => parsed["state"]["candidate_tokens"] = serde_json::Value::Null,
            3 => parsed["state"]["candidate_tokens"] = serde_json::json!({}),
            4 => {
                parsed["state"]["candidate_tokens"] = serde_json::json!({"C1":alternative.clone()})
            }
            5 => {
                parsed["state"]["candidate_tokens"]["C0"]["material_digest"] =
                    serde_json::json!("changed")
            }
            6 => parsed["model"] = serde_json::json!("unsupported"),
            7 => parsed["state"]["request"]["alternatives"][0]["id"] = serde_json::Value::Null,
            _ => unreachable!(),
        }
        assert!(
            fixture_v3_response(&parsed).is_err(),
            "malformed case {case}"
        );
    }
}

pub(in super::super) struct CountingCapableProvider(
    pub(in super::super) std::sync::Arc<std::sync::atomic::AtomicUsize>,
    pub(in super::super) std::sync::Arc<std::sync::atomic::AtomicUsize>,
);

#[async_trait::async_trait]
impl ScopeAdviceProvider for CountingCapableProvider {
    fn identity(&self) -> Option<(&'static str, &'static str)> {
        Some(("test-only", "fixture"))
    }

    fn prepare_context(
        &self,
        context: &tect_application::ScopeAdviceProviderContext,
    ) -> std::result::Result<PreparedScopeAdviceAttempt, ScopeAdviceProviderError> {
        let request = context.request();
        self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        PreparedScopeAdviceAttempt::new(
            request.clone(),
            b"{}".to_vec(),
            "fixture".into(),
            "jev".into(),
            "https://fixture.invalid".into(),
            "fixture.v1".into(),
        )
    }

    async fn attempt_prepared(
        &self,
        _: &ScopeAdviceProviderRequest,
        _: PreparedScopeAdviceAttempt,
        _: StartedScopeDispatchPermit,
    ) -> std::result::Result<ScopeAdviceProviderObservation, ScopeAdviceProviderError> {
        self.1.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Err(ScopeAdviceProviderError::ProvenNotSent)
    }
}

pub(in super::super) struct UnusedHostAdapters;

#[async_trait::async_trait]
impl SourceInspector for UnusedHostAdapters {
    async fn inspect(&self, _: &str, _: &[String]) -> Result<tect_domain::SourceLocation> {
        Err(Error::InternalInvariant)
    }
}

impl SetupFiles for UnusedHostAdapters {
    fn resolve_directory(&self, _: &str, _: &[String]) -> Result<tect_domain::SetupDirectory> {
        Err(Error::InternalInvariant)
    }

    fn inspect(
        &self,
        _: &tect_domain::SetupDirectory,
        _: usize,
    ) -> Result<tect_domain::FileObservation> {
        Err(Error::InternalInvariant)
    }

    fn publish(
        &self,
        _: &tect_domain::SetupDirectory,
        _: &str,
    ) -> Result<tect_domain::FilePublication> {
        Err(Error::InternalInvariant)
    }
}

pub(in super::super) struct FixtureCandidateGuidance;

pub(in super::super) struct FixtureCandidateOutputGuard;

impl tect_application::CandidateOutputGuard for FixtureCandidateOutputGuard {
    fn input_bytes(&self, input: &str) -> Result<i64> {
        Ok(input.len() as i64)
    }
    fn check_material(&self, _: &CandidateSnapshotMaterial) -> Result<()> {
        Ok(())
    }
    fn check_draft(&self, _: &ResolvedCandidateDraft) -> Result<()> {
        Ok(())
    }
    fn check_stored(&self, _: &StoredCandidateContext) -> Result<()> {
        Ok(())
    }
    fn check_begin(&self, _: &BeginCandidateSetOutcome) -> Result<()> {
        Ok(())
    }
}

impl tect_application::CandidateGuidance for FixtureCandidateGuidance {
    fn snapshot(
        &self,
        program: Program,
        selected_worktrees: Vec<WorktreeSummary>,
    ) -> Result<CandidateSnapshotMaterial> {
        Ok(CandidateSnapshotMaterial {
            program,
            selected_worktrees,
            selected_sources_digest: D.into(),
            method: CandidateMethodSnapshot {
                id: "m".into(),
                revision: "4".into(),
                digest: D.into(),
                body: "body".into(),
                origin_refs: vec![],
            },
            registry_revision: "3".into(),
            registry_digest: D.into(),
            rules: vec![],
        })
    }
}
