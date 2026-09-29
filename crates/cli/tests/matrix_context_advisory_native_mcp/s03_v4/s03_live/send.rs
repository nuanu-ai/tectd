use super::*;
use std::{
    fs,
    io::{BufRead, Write},
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::Path,
    sync::Arc,
};
use tect_application::{
    AdvisoryProviderReceiptObservation, AdvisoryProviderReceiptUsage, PipelineProviderObservation,
    PipelineRecommendationProvider, PipelineStartedDispatchPermit, PreparedPipelineRecommendation,
    PreparedPipelineRecommendationAttempt, SealedPipelineRecommendationResponse,
    StoredAdvisoryProviderReceipt,
};
use tect_domain::{Error, PipelineRecommendationRanking, Result};

pub(super) const CALL_ID: &str = "tectd-jev-pipeline-s03-effect-2026-09-29-7";

pub(super) fn artifact_paths() -> (std::path::PathBuf, std::path::PathBuf) {
    let dir = std::path::PathBuf::from(
        std::env::var("JEV_PIPELINE_ONE_SHOT_ARTIFACT_DIR")
            .expect("explicit retained artifact directory required for send"),
    );
    assert!(dir.is_absolute() && dir.is_dir());
    let dir = dir.canonicalize().unwrap();
    let checkout = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap()
        .canonicalize()
        .unwrap();
    assert!(
        !dir.starts_with(checkout),
        "one-use artifacts must be outside the checkout"
    );
    assert_eq!(
        fs::metadata(&dir).unwrap().permissions().mode() & 0o077,
        0,
        "artifact directory must be owner-only"
    );
    (
        dir.join(format!("{CALL_ID}.request.json")),
        dir.join(format!("{CALL_ID}.used")),
    )
}

fn exclusive_write(path: &Path, bytes: &[u8]) {
    let parent = path.parent().expect("artifact parent");
    assert!(path.is_absolute() && parent.is_dir());
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .expect("one-use artifact exists or cannot be created; no call sent");
    file.write_all(bytes).unwrap();
    file.sync_all().unwrap();
    fs::File::open(parent).unwrap().sync_all().unwrap();
}

pub(super) fn review_request(path: &Path, body: &[u8]) {
    exclusive_write(path, body);
    assert_eq!(fs::read(path).unwrap(), body);
}

pub(super) fn confirmation_matches(reader: &mut impl BufRead, digest: &str) -> bool {
    let mut answer = String::new();
    if reader.read_line(&mut answer).is_err() {
        return false;
    }
    let expected = format!("SEND JEV PIPELINE {digest}");
    answer
        .strip_suffix('\n')
        .is_some_and(|line| line.strip_suffix('\r').unwrap_or(line) == expected)
}

pub(super) fn mark_one_use(marker: &Path, digest: &str) {
    exclusive_write(
        marker,
        format!("call_id={CALL_ID}\nrequest_sha256={digest}\n").as_bytes(),
    );
}

/// Enforces the reviewed bytes again at the service provider boundary.
pub(super) struct ReviewedProvider {
    pub(super) inner: JevPipelineProvider,
    pub(super) reviewed: Arc<Vec<u8>>,
}

#[async_trait::async_trait]
impl PipelineRecommendationProvider for ReviewedProvider {
    fn prepare(
        &self,
        saved: &PreparedPipelineRecommendation,
    ) -> Result<PreparedPipelineRecommendationAttempt> {
        let prepared = self.inner.prepare(saved)?;
        if prepared.body() != self.reviewed.as_slice() {
            return Err(Error::InputConflict);
        }
        Ok(prepared)
    }

    async fn observe_prepared(
        &self,
        prepared: PreparedPipelineRecommendationAttempt,
        permit: PipelineStartedDispatchPermit,
    ) -> Result<AdvisoryProviderReceiptObservation> {
        if prepared.body() != self.reviewed.as_slice() {
            return Err(Error::InputConflict);
        }
        self.inner.observe_prepared(prepared, permit).await
    }

    fn usage_from_sealed_response(
        &self,
        saved: &StoredAdvisoryProviderReceipt,
    ) -> Result<AdvisoryProviderReceiptUsage> {
        self.inner.usage_from_sealed_response(saved)
    }

    fn parse_sealed_response(
        &self,
        manifest: &PipelineRecommendationManifest,
        prepared: &PreparedPipelineRecommendationAttempt,
        sealed: &SealedPipelineRecommendationResponse,
    ) -> Result<PipelineRecommendationRanking> {
        self.inner.parse_sealed_response(manifest, prepared, sealed)
    }

    async fn attempt_prepared(
        &self,
        prepared: PreparedPipelineRecommendationAttempt,
        permit: PipelineStartedDispatchPermit,
    ) -> Result<PipelineProviderObservation> {
        if prepared.body() != self.reviewed.as_slice() {
            return Err(Error::InputConflict);
        }
        self.inner.attempt_prepared(prepared, permit).await
    }
}

#[test]
fn confirmation_is_exact_and_marker_is_exclusive() {
    let dir = private_temp();
    let marker = dir.path().join("one.used");
    for line in ["", "SEND JEV PIPELINE abc", "SEND JEV PIPELINE wrong\n"] {
        assert!(!confirmation_matches(
            &mut std::io::Cursor::new(line),
            "abc"
        ));
        assert!(!marker.exists());
    }
    assert!(confirmation_matches(
        &mut std::io::Cursor::new("SEND JEV PIPELINE abc\n"),
        "abc"
    ));
    assert!(!marker.exists());
    mark_one_use(&marker, "abc");
    assert!(marker.exists());
}
