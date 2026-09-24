use super::lifecycle::{AdvisoryOpportunityState, AdvisoryReason};
use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const SCOPE_DECOMPOSITION_DECISION_POINT: &str = "scope.decomposition.before_selection";
pub const ENGINEERING_PROFILE_DECISION_POINT: &str = "engineering.profile.before_selection";
pub const ADVISORY_DECISION_POINT_VERSION: i32 = 1;
pub const ADVISORY_CONFIG_REVISION_DEFAULT: i64 = 0;
pub const ADVISORY_POLICY_VERSION: &str = "slice-00.v1";
pub(super) const MAX_ADVISORY_KEY_BYTES: usize = 256;
pub(super) const MAX_ADVISORY_TEXT_BYTES: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceAdvisoryMode {
    #[default]
    Disabled,
    Optional,
}

impl WorkspaceAdvisoryMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            Self::Optional => "optional",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum AdvisoryRequestPreference {
    #[default]
    UseWorkspace,
    Skip,
}

impl AdvisoryRequestPreference {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::UseWorkspace => "use_workspace",
            Self::Skip => "skip",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdvisoryCapability {
    ScopeDecomposition,
    EngineeringProfile,
    PipelineRecommendation,
    AntiBloat,
    ModelRouting,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AdvisoryDecisionPoint {
    #[serde(rename = "scope.decomposition.before_selection")]
    ScopeDecompositionBeforeSelection,
    #[serde(rename = "engineering.profile.before_selection")]
    EngineeringProfileBeforeSelection,
}

impl AdvisoryDecisionPoint {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ScopeDecompositionBeforeSelection => SCOPE_DECOMPOSITION_DECISION_POINT,
            Self::EngineeringProfileBeforeSelection => ENGINEERING_PROFILE_DECISION_POINT,
        }
    }

    pub const fn capability(self) -> AdvisoryCapability {
        match self {
            Self::ScopeDecompositionBeforeSelection => AdvisoryCapability::ScopeDecomposition,
            Self::EngineeringProfileBeforeSelection => AdvisoryCapability::EngineeringProfile,
        }
    }

    pub const fn supports(self, capability: AdvisoryCapability) -> bool {
        matches!(
            (self, capability),
            (
                Self::ScopeDecompositionBeforeSelection,
                AdvisoryCapability::ScopeDecomposition
            ) | (
                Self::EngineeringProfileBeforeSelection,
                AdvisoryCapability::EngineeringProfile
            )
        )
    }
}

impl std::fmt::Display for AdvisoryDecisionPoint {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl std::str::FromStr for AdvisoryDecisionPoint {
    type Err = Error;

    fn from_str(value: &str) -> Result<Self> {
        match value {
            SCOPE_DECOMPOSITION_DECISION_POINT => Ok(Self::ScopeDecompositionBeforeSelection),
            ENGINEERING_PROFILE_DECISION_POINT => Ok(Self::EngineeringProfileBeforeSelection),
            _ => Err(Error::InvalidArguments),
        }
    }
}

impl AdvisoryCapability {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ScopeDecomposition => "scope_decomposition",
            Self::EngineeringProfile => "engineering_profile",
            Self::PipelineRecommendation => "pipeline_recommendation",
            Self::AntiBloat => "anti_bloat",
            Self::ModelRouting => "model_routing",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceAdvisoryConfig {
    pub workspace_id: Uuid,
    pub revision: i64,
    pub mode: WorkspaceAdvisoryMode,
    /// False only for the read-only disabled/revision-0 projection.
    pub materialized: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_profile_ref: Option<AdvisoryProviderProfileRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_configuration: Option<AdvisoryModelConfiguration>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdvisoryProviderProfileRef {
    pub id: String,
}

impl AdvisoryProviderProfileRef {
    pub fn validate(&self) -> Result<()> {
        validate_non_secret_identifier(&self.id)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdvisoryModelConfiguration {
    pub model: String,
}

impl AdvisoryModelConfiguration {
    pub fn validate(&self) -> Result<()> {
        validate_non_secret_identifier(&self.model)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfigureWorkspaceAdvisory {
    pub expected_revision: i64,
    pub mode: WorkspaceAdvisoryMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_profile_ref: Option<AdvisoryProviderProfileRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_configuration: Option<AdvisoryModelConfiguration>,
}

impl ConfigureWorkspaceAdvisory {
    pub fn validate(&self) -> Result<()> {
        if self.expected_revision < 0
            || self.provider_profile_ref.is_some() != self.model_configuration.is_some()
        {
            return Err(Error::InvalidArguments);
        }
        if let Some(profile) = &self.provider_profile_ref {
            profile.validate()?;
        }
        if let Some(model) = &self.model_configuration {
            model.validate()?;
        }
        Ok(())
    }
}

impl WorkspaceAdvisoryConfig {
    pub fn provider_configured(&self) -> bool {
        self.provider_profile_ref.is_some() && self.model_configuration.is_some()
    }
}

pub fn next_advisory_config_revision(current: Option<i64>, expected: i64) -> Result<i64> {
    if expected < ADVISORY_CONFIG_REVISION_DEFAULT || current.unwrap_or(0) != expected {
        return Err(Error::StaleRevision);
    }
    expected.checked_add(1).ok_or(Error::InvalidArguments)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AdvisoryPolicyInput {
    pub workspace_mode: WorkspaceAdvisoryMode,
    pub session_preference: AdvisoryRequestPreference,
    pub request_preference: AdvisoryRequestPreference,
    pub deterministic_input_valid: bool,
    pub capability_available: bool,
    pub provider_configured: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AdvisoryPolicyDecision {
    pub state: AdvisoryOpportunityState,
    pub reason: AdvisoryReason,
}

pub const fn assess_advisory_policy(input: AdvisoryPolicyInput) -> AdvisoryPolicyDecision {
    let reason = if matches!(input.workspace_mode, WorkspaceAdvisoryMode::Disabled) {
        AdvisoryReason::WorkspaceDisabled
    } else if matches!(input.session_preference, AdvisoryRequestPreference::Skip) {
        AdvisoryReason::SessionSkip
    } else if matches!(input.request_preference, AdvisoryRequestPreference::Skip) {
        AdvisoryReason::RequestSkip
    } else if !input.deterministic_input_valid {
        AdvisoryReason::DeterministicInputInvalid
    } else if !input.capability_available {
        AdvisoryReason::CapabilityUnavailable
    } else if !input.provider_configured {
        AdvisoryReason::ProviderUnconfigured
    } else {
        AdvisoryReason::DispatchAuthorized
    };
    AdvisoryPolicyDecision {
        state: if matches!(reason, AdvisoryReason::DispatchAuthorized) {
            AdvisoryOpportunityState::Prepared
        } else {
            AdvisoryOpportunityState::NoCall
        },
        reason,
    }
}

pub fn assess_advisory_opportunity(
    mode: WorkspaceAdvisoryMode,
    preference: AdvisoryRequestPreference,
) -> AdvisoryPolicyDecision {
    assess_advisory_policy(AdvisoryPolicyInput {
        workspace_mode: mode,
        session_preference: AdvisoryRequestPreference::UseWorkspace,
        request_preference: preference,
        deterministic_input_valid: true,
        capability_available: false,
        provider_configured: false,
    })
}

pub(super) fn validate_non_secret_identifier(value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > MAX_ADVISORY_TEXT_BYTES
        || value.contains('\0')
        || value.trim() != value
    {
        return Err(Error::InvalidArguments);
    }
    Ok(())
}
