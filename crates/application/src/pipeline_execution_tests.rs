use super::*;
use async_trait::async_trait;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use tect_domain::{
    FileObservation, FilePublication, HostAuth, PipelineSourceAmendment,
    PipelineSourceArtifactDraft, PipelineSourcePredecessor, PipelineSourceSuccessor,
    RequestContext, SetupDirectory, SourceLocation,
};

struct CountingStore(AtomicUsize);

#[async_trait]
impl crate::Store for CountingStore {
    async fn begin(&self, _mode: TransactionMode) -> Result<Box<dyn crate::UnitOfWork>> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Err(Error::InternalInvariant)
    }
}

struct UnusedAdapters;

#[async_trait]
impl crate::SourceInspector for UnusedAdapters {
    async fn inspect(&self, _path: &str, _allowed_roots: &[String]) -> Result<SourceLocation> {
        Err(Error::InternalInvariant)
    }
}

impl crate::SetupFiles for UnusedAdapters {
    fn resolve_directory(&self, _path: &str, _current_roots: &[String]) -> Result<SetupDirectory> {
        Err(Error::InternalInvariant)
    }

    fn inspect(&self, _directory: &SetupDirectory, _max_bytes: usize) -> Result<FileObservation> {
        Err(Error::InternalInvariant)
    }

    fn publish(&self, _directory: &SetupDirectory, _content: &str) -> Result<FilePublication> {
        Err(Error::InternalInvariant)
    }
}

fn amendment_request(body: &str, declared_digest: &str) -> RecordPipelineInput {
    RecordPipelineInput {
        request_id: Uuid::new_v4(),
        run_id: Uuid::new_v4(),
        run_revision: 2,
        phase_id: "slice-implementation-spec-synthesizer".to_owned(),
        input: "Direct source amendment authority.".to_owned(),
        source_amendment: Some(PipelineSourceAmendment {
            target_phase_id: "slice-component-decision-interrogator".to_owned(),
            predecessor: PipelineSourcePredecessor {
                output_id: Uuid::new_v4(),
                output_revision: 1,
                output_digest: "a".repeat(64),
                artifact_name: "requirements-ledger.json".to_owned(),
                artifact_digest: "b".repeat(64),
                source_path: "source.md".to_owned(),
                source_digest: "c".repeat(64),
            },
            successor: PipelineSourceSuccessor {
                path: "source.md".to_owned(),
                artifact: PipelineSourceArtifactDraft {
                    name: "source.md".to_owned(),
                    media_type: "text/markdown".to_owned(),
                    body: body.to_owned(),
                    digest: declared_digest.to_owned(),
                    reference: None,
                },
            },
            authorization_scope: "Amend the current Full Design source.".to_owned(),
            authorization_provenance: "Exact direct operator input.".to_owned(),
        }),
    }
}

#[test]
fn forged_declared_digest_cannot_mint_an_attestation() {
    let request = amendment_request("arbitrary body", &"0".repeat(64));
    request.validate().expect("shape is valid");
    let amendment = request.source_amendment.as_ref().unwrap();
    let actual_digest = Sha256::digest(amendment.successor.artifact.body.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();

    assert!(matches!(
        VerifiedPipelineSourceDigest::new(
            request.request_id,
            &amendment.successor.artifact.digest,
            actual_digest,
        ),
        Err(Error::InvalidArguments)
    ));
}

#[tokio::test]
async fn public_service_rejects_forged_digest_before_entering_store() {
    let store = Arc::new(CountingStore(AtomicUsize::new(0)));
    let service = WorkspaceService::new(
        store.clone(),
        Arc::new(UnusedAdapters),
        Arc::new(UnusedAdapters),
    );
    let context = RequestContext {
        auth: HostAuth {
            host_id: Uuid::new_v4(),
            credential: "a".repeat(64),
        },
        native_session_id: "native-session".to_owned(),
        workspace_key: "workspace".to_owned(),
    };
    let request = amendment_request("arbitrary body", &"0".repeat(64));

    assert!(matches!(
        service.pipeline_input_record(&context, &request).await,
        Err(Error::InvalidArguments)
    ));
    assert_eq!(store.0.load(Ordering::SeqCst), 0);
}

#[test]
fn attestation_is_bound_to_request_and_actual_digest() {
    let body = "amended body";
    let digest = Sha256::digest(body.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let request = amendment_request(body, &digest);
    let verified = VerifiedPipelineSourceDigest::new(request.request_id, &digest, digest.clone())
        .expect("application-computed digest matches declaration");

    assert_eq!(verified.request_id(), request.request_id);
    assert_eq!(verified.digest(), digest);
}
