use crate::{storage_error, store::PgUnitOfWork};
use async_trait::async_trait;
use sqlx::{Postgres, Transaction};
use tect_application::{
    GuardedScopeAdviceRecord, ScopeAdvisoryStore, ScopeAuthoredManifestRequest,
    ScopeAuthorityObservation, ScopeAuthorityObserver, ScopeAuthorityOutcome,
    ScopeAuthorityRequest, ScopeAuthorizedInvalidObservation, ScopeCallerLinkInput,
    ScopeDispositionRecord, ScopeManifestRecord, ScopeManifestSupplier,
    ScopePreparedAdvisoryDisposition, ScopePreservationReceiptInput, ScopeVerifierReceiptInput,
    Sha256ScopeDigest, StoredScopeManifestRecord,
};
use tect_domain::*;
use uuid::Uuid;

include!("scope_advisory/mappings.rs");
include!("scope_advisory/manifest.rs");
include!("scope_advisory/authority.rs");
include!("scope_advisory/authored_supplier.rs");
include!("scope_advisory/decisions.rs");
include!("scope_advisory/disposition_preservation.rs");
include!("scope_advisory/caller_verifier.rs");
include!("scope_advisory/caller_save.rs");
include!("scope_advisory/selected_save_observation.rs");
include!("scope_advisory/finalize.rs");
include!("scope_advisory/store.rs");

#[cfg(test)]
mod anti_bloat_live_tests;
#[cfg(test)]
mod live_support;
#[cfg(test)]
mod live_tests;
#[cfg(test)]
mod receipt_recovery_test_support;
