//! Bounded public-entry denial proof. No UnitOfWork is fabricated here: these
//! tests do not establish successful authentication, workspace ACL, ancestry,
//! operating verification, or a successful combined public comparison.
use super::*;
use async_trait::async_trait;
use std::sync::{Arc, Mutex};
use tect_domain::{
    FileObservation, FilePublication, HostAuth, RequestContext, SetupDirectory, SourceLocation,
};

struct BeginFailureStore {
    modes: Mutex<Vec<TransactionMode>>,
    error: Error,
}

#[async_trait]
impl crate::Store for BeginFailureStore {
    async fn begin(&self, mode: TransactionMode) -> Result<Box<dyn crate::UnitOfWork>> {
        self.modes.lock().unwrap().push(mode);
        Err(self.error.clone())
    }
}

struct ForbiddenAdapters;

#[async_trait]
impl crate::SourceInspector for ForbiddenAdapters {
    async fn inspect(&self, _: &str, _: &[String]) -> Result<SourceLocation> {
        panic!("technical comparison must not inspect source paths")
    }
}

impl crate::SetupFiles for ForbiddenAdapters {
    fn resolve_directory(&self, _: &str, _: &[String]) -> Result<SetupDirectory> {
        panic!("technical comparison must not resolve setup directories")
    }
    fn inspect(&self, _: &SetupDirectory, _: usize) -> Result<FileObservation> {
        panic!("technical comparison must not inspect setup files")
    }
    fn publish(&self, _: &SetupDirectory, _: &str) -> Result<FilePublication> {
        panic!("technical comparison must not publish files")
    }
}

#[async_trait]
impl crate::TechnicalDecisionEvidenceResolver for ForbiddenAdapters {
    async fn resolve(
        &self,
        _: &crate::ServerTechnicalDecisionTaskBinding,
        _: &crate::TechnicalDecisionEvidenceReference,
        _: i64,
    ) -> Result<Option<crate::ResolvedTechnicalDecisionEvidence>> {
        panic!("entry denial must precede evidence resolution")
    }
}

#[async_trait]
impl crate::MatrixAdviceProvider for ForbiddenAdapters {
    fn identity(&self) -> Option<crate::MatrixProviderIdentity> {
        panic!("technical comparison must not query a ranking provider")
    }
    fn prepare(
        &self,
        _: &crate::MatrixProviderRequest,
    ) -> Result<crate::PreparedMatrixAdviceAttempt> {
        panic!("technical comparison must not prepare provider requests")
    }
    async fn attempt_prepared(
        &self,
        _: crate::PreparedMatrixAdviceAttempt,
        _: crate::MatrixStartedDispatchPermit,
    ) -> Result<crate::MatrixProviderResponse> {
        panic!("technical comparison must not dispatch a provider")
    }
}

fn fixture(error: Error) -> (WorkspaceService, Arc<BeginFailureStore>, RequestContext) {
    let store = Arc::new(BeginFailureStore {
        modes: Mutex::new(Vec::new()),
        error,
    });
    let guards = Arc::new(ForbiddenAdapters);
    let mut service = WorkspaceService::new(store.clone(), guards.clone(), guards.clone())
        .with_technical_decision_evidence_resolver(guards.clone());
    service.matrix_advice_provider = guards;
    let context = RequestContext {
        auth: HostAuth {
            host_id: Uuid::new_v4(),
            credential: "a".repeat(64),
        },
        native_session_id: Uuid::new_v4().to_string(),
        workspace_key: "offline-proof".into(),
    };
    (service, store, context)
}

fn request() -> CompareTechnicalDeliveryMechanisms {
    CompareTechnicalDeliveryMechanisms {
        task_id: Uuid::new_v4(),
        expected_task_revision: 1,
        operating_verification_digest: "b".repeat(64),
        evidence_reference: crate::TechnicalDecisionEvidenceReference {
            artifact_id: Uuid::new_v4(),
            artifact_version: 1,
            content_sha256: "c".repeat(64),
        },
    }
}

#[tokio::test]
async fn public_comparison_rejects_malformed_pins_before_store_or_resolver() {
    let (service, store, context) = fixture(Error::InternalInvariant);
    for field in 0..8 {
        let mut request = request();
        match field {
            0 => request.task_id = Uuid::nil(),
            1 => request.expected_task_revision = 0,
            2 => request.operating_verification_digest = "b".repeat(63),
            3 => request.operating_verification_digest = "g".repeat(64),
            4 => request.evidence_reference.artifact_id = Uuid::nil(),
            5 => request.evidence_reference.artifact_version = 0,
            6 => request.evidence_reference.content_sha256 = "c".repeat(63),
            7 => request.evidence_reference.content_sha256 = "g".repeat(64),
            _ => unreachable!(),
        }
        assert_eq!(
            service
                .compare_technical_delivery_mechanisms(&context, &request)
                .await,
            Err(Error::InvalidArguments),
            "malformed field {field}"
        );
    }
    assert!(store.modes.lock().unwrap().is_empty());
}

#[tokio::test]
async fn public_comparison_rejects_invalid_auth_context_before_store() {
    let (service, store, context) = fixture(Error::InternalInvariant);
    for field in 0..3 {
        let mut context = context.clone();
        match field {
            0 => context.auth.host_id = Uuid::nil(),
            1 => context.auth.credential = "a".repeat(63),
            2 => context.auth.credential = "g".repeat(64),
            _ => unreachable!(),
        }
        assert_eq!(
            service
                .compare_technical_delivery_mechanisms(&context, &request())
                .await,
            Err(Error::Unauthorized)
        );
    }
    assert!(store.modes.lock().unwrap().is_empty());
}

#[tokio::test]
async fn public_comparison_propagates_begin_failure_without_evidence_or_dispatch() {
    for error in [Error::StorageUnavailable, Error::InternalInvariant] {
        let (service, store, context) = fixture(error.clone());
        assert_eq!(
            service
                .compare_technical_delivery_mechanisms(&context, &request())
                .await,
            Err(error)
        );
        assert_eq!(*store.modes.lock().unwrap(), [TransactionMode::ReadWrite]);
        // No UnitOfWork exists: authenticate, persistence and commit cannot be
        // reached. This is transaction-entry failure, not store-auth proof.
    }
}
