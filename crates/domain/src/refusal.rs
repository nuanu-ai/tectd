use serde::{Deserialize, Serialize};

/// Stable, machine-readable reasons for a request being refused before its
/// protected effect.  The legacy `Error` code remains alongside this contract
/// at API boundaries for compatibility.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RefusalCode {
    StaleRevision,
    IdempotencyConflict,
    InvalidOutput,
    PayloadTooLarge,
    EvidenceMissing,
    ArtifactNotReady,
    AmbiguousRequirement,
    /// The submitted value does not satisfy the selected public input schema.
    InputSchemaInvalid,
    UnknownCause,
    NoTestTarget,
    ReviewRequired,
    AuthorityRequired,
    MethodVersionUnavailable,
    DeliveryRefreshRequired,
    CoverageIncomplete,
    DependencyStale,
    EffectStatusUnknown,
    /// Proof that is owned by the backend cannot be supplied or replayed by
    /// an agent as part of a new pipeline completion.
    BackendDerivedProofRequired,
    /// A legacy run cannot be reinterpreted as a newer definition.  Callers
    /// must provide an explicit successor and obligation/evidence mapping.
    LegacyMigrationRequired,
}

impl RefusalCode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::StaleRevision => "STALE_REVISION",
            Self::IdempotencyConflict => "IDEMPOTENCY_CONFLICT",
            Self::InvalidOutput => "INVALID_OUTPUT",
            Self::PayloadTooLarge => "PAYLOAD_TOO_LARGE",
            Self::EvidenceMissing => "EVIDENCE_MISSING",
            Self::ArtifactNotReady => "ARTIFACT_NOT_READY",
            Self::AmbiguousRequirement => "AMBIGUOUS_REQUIREMENT",
            Self::InputSchemaInvalid => "INPUT_SCHEMA_INVALID",
            Self::UnknownCause => "UNKNOWN_CAUSE",
            Self::NoTestTarget => "NO_TEST_TARGET",
            Self::ReviewRequired => "REVIEW_REQUIRED",
            Self::AuthorityRequired => "AUTHORITY_REQUIRED",
            Self::MethodVersionUnavailable => "METHOD_VERSION_UNAVAILABLE",
            Self::DeliveryRefreshRequired => "DELIVERY_REFRESH_REQUIRED",
            Self::CoverageIncomplete => "COVERAGE_INCOMPLETE",
            Self::DependencyStale => "DEPENDENCY_STALE",
            Self::EffectStatusUnknown => "EFFECT_STATUS_UNKNOWN",
            Self::BackendDerivedProofRequired => "BACKEND_DERIVED_PROOF_REQUIRED",
            Self::LegacyMigrationRequired => "LEGACY_MIGRATION_REQUIRED",
        }
    }

    /// Stable human-readable explanation paired with the machine code.  This
    /// is deliberately independent of transport wording so clients do not
    /// need to reverse engineer a recovery reason from `error.code`.
    pub const fn message(self) -> &'static str {
        match self {
            Self::StaleRevision => "the supplied revision is no longer current",
            Self::IdempotencyConflict => "the request identity is already bound to different input",
            Self::InvalidOutput => "the submitted pipeline output violates its contract",
            Self::PayloadTooLarge => "the submitted pipeline payload exceeds its bounded limit",
            Self::EvidenceMissing => {
                "required pipeline evidence is missing or outside the required scope"
            }
            Self::ArtifactNotReady => "a referenced pipeline artifact is not ready for consumption",
            Self::AmbiguousRequirement => {
                "the request does not identify one valid pipeline requirement"
            }
            Self::InputSchemaInvalid => {
                "the submitted value does not satisfy the selected input schema"
            }
            Self::UnknownCause => {
                "the pipeline cannot establish a safe cause for the requested continuation"
            }
            Self::NoTestTarget => "the pipeline has no executable test target",
            Self::ReviewRequired => "the required review has not produced a ready decision",
            Self::AuthorityRequired => {
                "the current principal or environment lacks required authority"
            }
            Self::MethodVersionUnavailable => {
                "the requested pipeline method version is unavailable"
            }
            Self::DeliveryRefreshRequired => {
                "the delivered pipeline context must be refreshed before continuation"
            }
            Self::CoverageIncomplete => "the pipeline coverage contract is incomplete",
            Self::DependencyStale => "a consumed pipeline dependency is stale",
            Self::EffectStatusUnknown => "the protected effect status is unknown",
            Self::BackendDerivedProofRequired => {
                "pipeline proof must be derived from backend-owned records"
            }
            Self::LegacyMigrationRequired => {
                "the legacy run requires an explicit successor migration"
            }
        }
    }
}

/// Compact refusal metadata. Optional fields are omitted to keep the common
/// error response small while allowing callers to recover deterministically.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct Refusal {
    pub code: RefusalCode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_action: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub required: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rule: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actual: Option<String>,
}

impl Refusal {
    pub const fn new(code: RefusalCode) -> Self {
        Self {
            code,
            message: None,
            next_action: None,
            required: None,
            revision: None,
            rule: None,
            path: None,
            expected: None,
            actual: None,
        }
    }

    pub fn with_message(mut self, message: impl Into<String>) -> Self {
        self.message = Some(message.into());
        self
    }

    pub fn with_next_action(mut self, action: impl Into<String>) -> Self {
        self.next_action = Some(action.into());
        self
    }

    pub fn with_required(mut self, required: impl Into<String>) -> Self {
        self.required = Some(required.into());
        self
    }

    pub const fn with_revision(mut self, revision: i64) -> Self {
        self.revision = Some(revision);
        self
    }

    pub fn with_rule(mut self, rule: impl Into<String>) -> Self {
        self.rule = Some(rule.into());
        self
    }

    pub fn with_path(mut self, path: impl Into<String>) -> Self {
        self.path = Some(path.into());
        self
    }

    pub fn with_expected(mut self, expected: impl Into<String>) -> Self {
        self.expected = Some(expected.into());
        self
    }

    pub fn with_actual(mut self, actual: impl Into<String>) -> Self {
        self.actual = Some(actual.into());
        self
    }

    /// Complete the strict pipeline refusal envelope without replacing more
    /// precise metadata already assigned by the rejecting branch.
    pub fn normalize_pipeline(
        mut self,
        rule: impl Into<String>,
        path: impl Into<String>,
        expected: impl Into<String>,
        actual: impl Into<String>,
        fallback_next_action: impl Into<String>,
        fallback_required: impl Into<String>,
    ) -> Self {
        if self.message.is_none() {
            self.message = Some(self.code.message().to_owned());
        }
        if self.rule.is_none() {
            self.rule = Some(rule.into());
        }
        if self.path.is_none() {
            self.path = Some(path.into());
        }
        if self.expected.is_none() {
            self.expected = Some(expected.into());
        }
        if self.actual.is_none() {
            self.actual = Some(actual.into());
        }
        if self.next_action.is_none() {
            self.next_action = Some(fallback_next_action.into());
        }
        if self.required.is_none() {
            self.required = Some(fallback_required.into());
        }
        self
    }

    pub fn is_complete_pipeline_refusal(&self) -> bool {
        self.message.as_ref().is_some_and(|value| !value.is_empty())
            && self
                .next_action
                .as_ref()
                .is_some_and(|value| !value.is_empty())
            && self
                .required
                .as_ref()
                .is_some_and(|value| !value.is_empty())
            && self.rule.as_ref().is_some_and(|value| !value.is_empty())
            && self.path.as_ref().is_some_and(|value| !value.is_empty())
            && self
                .expected
                .as_ref()
                .is_some_and(|value| !value.is_empty())
            && self.actual.as_ref().is_some_and(|value| !value.is_empty())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refusal_serializes_compactly_and_round_trips() {
        let refusal = Refusal::new(RefusalCode::StaleRevision)
            .with_next_action("refresh")
            .with_required("revision")
            .with_revision(7);
        let value = serde_json::to_value(&refusal).unwrap();
        assert_eq!(value["code"], "STALE_REVISION");
        assert_eq!(value["next_action"], "refresh");
        assert_eq!(value["required"], "revision");
        assert_eq!(value["revision"], 7);
        assert_eq!(serde_json::from_value::<Refusal>(value).unwrap(), refusal);
    }

    #[test]
    fn omitted_optional_fields_stay_omitted() {
        let value = serde_json::to_value(Refusal::new(RefusalCode::UnknownCause)).unwrap();
        assert_eq!(value, serde_json::json!({"code":"UNKNOWN_CAUSE"}));
    }
}
