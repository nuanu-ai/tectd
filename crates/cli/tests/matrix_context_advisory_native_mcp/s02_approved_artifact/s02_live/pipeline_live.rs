//! Test-only provider switch: the authorized capture session stays open after review.
use super::*;
use std::{
    fs,
    io::{BufRead, Write},
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    sync::Mutex,
};
use tect_application::{
    AdvisoryProviderReceiptObservation, AdvisoryProviderReceiptUsage,
    AdvisoryProviderTransportContext, PipelineProviderObservation, PipelineRecommendationProvider,
    PipelineStartedDispatchPermit, PreparedPipelineRecommendation,
    PreparedPipelineRecommendationAttempt, SealedPipelineRecommendationResponse,
    StoredAdvisoryProviderReceipt,
};
use tect_domain::{
    AdvisoryDispatchOutcome, AdvisorySendCertainty, Error, PipelineRecommendationManifest,
    PipelineRecommendationRanking, Result,
};

pub(super) const CALL_ID: &str = "tectd-jev-pipeline-s03-active-mvp-2026-09-30-1";

pub(super) fn artifact_paths() -> (PathBuf, PathBuf) {
    let dir = PathBuf::from(
        std::env::var("JEV_PIPELINE_ONE_SHOT_ARTIFACT_DIR")
            .expect("explicit private artifact directory required"),
    );
    assert!(dir.is_absolute() && dir.is_dir());
    let dir = dir.canonicalize().unwrap();
    let checkout = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap()
        .canonicalize()
        .unwrap();
    assert!(!dir.starts_with(checkout));
    assert_eq!(fs::metadata(&dir).unwrap().permissions().mode() & 0o077, 0);
    (
        dir.join(format!("{CALL_ID}.request.json")),
        dir.join(format!("{CALL_ID}.used")),
    )
}

fn exclusive_write(path: &Path, bytes: &[u8]) {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .expect("one-use artifact already exists");
    file.write_all(bytes).unwrap();
    file.sync_all().unwrap();
    fs::File::open(path.parent().unwrap())
        .unwrap()
        .sync_all()
        .unwrap();
}

pub(super) fn review_request(path: &Path, bytes: &[u8]) {
    exclusive_write(path, bytes);
    assert_eq!(fs::read(path).unwrap(), bytes);
}

pub(super) fn mark(marker: &Path, digest: &str) {
    exclusive_write(
        marker,
        format!("call_id={CALL_ID}\nrequest_sha256={digest}\n").as_bytes(),
    );
}

pub(super) fn confirm_send(reader: &mut impl BufRead, digest: &str) -> bool {
    let mut line = String::new();
    reader.read_line(&mut line).is_ok()
        && line.strip_suffix('\n').is_some_and(|value| {
            value.strip_suffix('\r').unwrap_or(value) == format!("SEND JEV PIPELINE {digest}")
        })
}

pub(super) fn confirm_selection(
    reader: &mut impl BufRead,
    manifest_digest: &str,
    choice_id: &str,
) -> bool {
    let mut line = String::new();
    reader.read_line(&mut line).is_ok()
        && line.strip_suffix('\n').is_some_and(|value| {
            value.strip_suffix('\r').unwrap_or(value)
                == format!("SELECT JEV PIPELINE {manifest_digest} {choice_id}")
        })
}

#[test]
fn exact_gates_do_not_accept_absence_or_other_choice() {
    assert!(!confirm_send(&mut std::io::Cursor::new(""), "sha"));
    assert!(!confirm_send(
        &mut std::io::Cursor::new("SEND JEV PIPELINE other\n"),
        "sha"
    ));
    assert!(!confirm_send(
        &mut std::io::Cursor::new("SEND JEV PIPELINE sha  \n"),
        "sha"
    ));
    assert!(confirm_send(
        &mut std::io::Cursor::new("SEND JEV PIPELINE sha\n"),
        "sha"
    ));
    assert!(!confirm_selection(
        &mut std::io::Cursor::new("SELECT JEV PIPELINE manifest wrong\n"),
        "manifest",
        "chosen"
    ));
    assert!(confirm_selection(
        &mut std::io::Cursor::new("SELECT JEV PIPELINE manifest chosen\n"),
        "manifest",
        "chosen"
    ));
    let dir = private_temp();
    let marker = dir.path().join("one.used");
    mark(&marker, "sha");
    assert_eq!(
        fs::read_to_string(&marker).unwrap(),
        format!("call_id={CALL_ID}\nrequest_sha256=sha\n")
    );
    assert_eq!(
        fs::metadata(&marker).unwrap().permissions().mode() & 0o777,
        0o600
    );
}

type ActivatedProvider = (Arc<dyn PipelineRecommendationProvider>, Arc<Vec<u8>>);

pub(super) struct SwitchedPipelineProvider {
    pub parser: JevPipelineSavedResponseParser,
    pub live: Mutex<Option<ActivatedProvider>>,
}

impl SwitchedPipelineProvider {
    pub fn activate(
        &self,
        provider: Arc<dyn PipelineRecommendationProvider>,
        reviewed: Arc<Vec<u8>>,
    ) {
        let mut live = self.live.lock().unwrap();
        assert!(live.is_none(), "provider may activate once only");
        *live = Some((provider, reviewed));
    }

    fn live(&self) -> Result<ActivatedProvider> {
        self.live.lock().unwrap().clone().ok_or(Error::Forbidden)
    }
}

#[async_trait]
impl PipelineRecommendationProvider for SwitchedPipelineProvider {
    fn prepare(
        &self,
        saved: &PreparedPipelineRecommendation,
    ) -> Result<PreparedPipelineRecommendationAttempt> {
        self.parser.prepare(saved)
    }

    fn parse_sealed_response(
        &self,
        manifest: &PipelineRecommendationManifest,
        prepared: &PreparedPipelineRecommendationAttempt,
        sealed: &SealedPipelineRecommendationResponse,
    ) -> Result<PipelineRecommendationRanking> {
        self.live()?
            .0
            .parse_sealed_response(manifest, prepared, sealed)
    }

    fn usage_from_sealed_response(
        &self,
        saved: &StoredAdvisoryProviderReceipt,
    ) -> Result<AdvisoryProviderReceiptUsage> {
        self.live()?.0.usage_from_sealed_response(saved)
    }

    async fn observe_prepared(
        &self,
        prepared: PreparedPipelineRecommendationAttempt,
        permit: PipelineStartedDispatchPermit,
    ) -> Result<AdvisoryProviderReceiptObservation> {
        let (provider, reviewed) = self.live()?;
        if prepared.body() != reviewed.as_slice() {
            return Err(Error::InputConflict);
        }
        provider.observe_prepared(prepared, permit).await
    }

    async fn attempt_prepared(
        &self,
        prepared: PreparedPipelineRecommendationAttempt,
        permit: PipelineStartedDispatchPermit,
    ) -> Result<PipelineProviderObservation> {
        let (provider, reviewed) = self.live()?;
        if prepared.body() != reviewed.as_slice() {
            return Err(Error::InputConflict);
        }
        provider.attempt_prepared(prepared, permit).await
    }
}

/// Signed application dispatch with a controlled native answer; no transport exists.
pub(super) struct SyntheticPipelineProvider {
    pub parser: JevPipelineSavedResponseParser,
    pub abstain: bool,
}

#[async_trait]
impl PipelineRecommendationProvider for SyntheticPipelineProvider {
    fn prepare(
        &self,
        saved: &PreparedPipelineRecommendation,
    ) -> Result<PreparedPipelineRecommendationAttempt> {
        self.parser.prepare(saved)
    }

    fn parse_sealed_response(
        &self,
        manifest: &PipelineRecommendationManifest,
        prepared: &PreparedPipelineRecommendationAttempt,
        sealed: &SealedPipelineRecommendationResponse,
    ) -> Result<PipelineRecommendationRanking> {
        self.parser
            .parse_sealed_response(manifest, prepared, sealed)
    }

    async fn observe_prepared(
        &self,
        prepared: PreparedPipelineRecommendationAttempt,
        permit: PipelineStartedDispatchPermit,
    ) -> Result<AdvisoryProviderReceiptObservation> {
        if !permit.permits(&prepared) {
            return Err(Error::InputConflict);
        }
        let request: Value =
            serde_json::from_slice(prepared.body()).map_err(|_| Error::InputConflict)?;
        let eligible = request["state"]["manifest"]["options"]
            .as_array()
            .ok_or(Error::InputConflict)?
            .iter()
            .map(|option| {
                option["id"]
                    .as_str()
                    .map(str::to_owned)
                    .ok_or(Error::InputConflict)
            })
            .collect::<Result<Vec<_>>>()?;
        let mut raw: Value =
            serde_json::from_slice(&crate::s03_v4::native_response(prepared.body(), &eligible))
                .map_err(|_| Error::InputConflict)?;
        if self.abstain {
            raw["answers"]["choice_v1"]["choice"] = json!("ABSTAIN");
            let probabilities = raw["answers"]["choice_v1"]["probabilities"]
                .as_object_mut()
                .ok_or(Error::InputConflict)?;
            for (id, probability) in probabilities {
                *probability = json!(if id == "ABSTAIN" { 1.0 } else { 0.0 });
            }
        }
        let raw = serde_json::to_vec(&raw).map_err(|_| Error::InputConflict)?;
        Ok(AdvisoryProviderReceiptObservation {
            response_payload: Some(raw),
            http_status: Some(200),
            input_tokens: Some(20),
            output_tokens: Some(30),
            response_complete: true,
            original_transport_context: Some(AdvisoryProviderTransportContext {
                send_certainty: AdvisorySendCertainty::Sent,
                outcome: AdvisoryDispatchOutcome::ProviderResponse,
                raw_response_ref: None,
                provider_failure_code: None,
            }),
        })
    }

    async fn attempt_prepared(
        &self,
        _prepared: PreparedPipelineRecommendationAttempt,
        _permit: PipelineStartedDispatchPermit,
    ) -> Result<PipelineProviderObservation> {
        Err(Error::Forbidden)
    }
}
