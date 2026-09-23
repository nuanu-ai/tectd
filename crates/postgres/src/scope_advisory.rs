use crate::{storage_error, store::PgUnitOfWork};
use async_trait::async_trait;
use sqlx::{Postgres, Transaction};
use tect_application::{
    GuardedScopeAdviceRecord, ScopeAdvisoryStore, ScopeAuthorityObservation,
    ScopeAuthorityObserver, ScopeAuthorityOutcome, ScopeAuthorityRequest,
    ScopeAuthorizedInvalidObservation, ScopeCallerLinkInput, ScopeDispositionRecord,
    ScopeManifestRecord, ScopePreservationReceiptInput, ScopeVerifierReceiptInput,
    Sha256ScopeDigest,
};
use tect_domain::*;
use uuid::Uuid;

include!("scope_advisory/mappings.rs");
include!("scope_advisory/manifest.rs");
include!("scope_advisory/authority.rs");
include!("scope_advisory/decisions.rs");
include!("scope_advisory/finalize.rs");
include!("scope_advisory/store.rs");

#[cfg(test)]
mod live_support;
#[cfg(test)]
mod live_tests;
