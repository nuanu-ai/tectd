use serde_json::Value;
use tect_application::{AuthoredScopeSet, RunScopeAdvisory, VerifySelectedSave};
use tect_domain::{
    AdvisoryAuditQuery, AdvisoryCapability, AdvisoryDecisionPoint, AdvisoryOpportunityState,
    AdvisoryReason, AdvisoryRequestPreference, ConfigureWorkspaceAdvisory, Error, Result,
    ScopeAdviceId, ScopeAlternativeId, ScopeDispositionAction, ScopeDispositionItem,
    ScopeDispositionRequest,
};

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct WorkspaceAuditArguments {
    limit: u32,
    #[serde(default)]
    scope_id: Option<uuid::Uuid>,
    #[serde(default)]
    after: Option<uuid::Uuid>,
    #[serde(default)]
    capability: Option<AdvisoryCapability>,
    #[serde(default)]
    decision_point: Option<AdvisoryDecisionPoint>,
    #[serde(default)]
    reason: Option<AdvisoryReason>,
    #[serde(default)]
    state: Option<AdvisoryOpportunityState>,
}

impl WorkspaceAuditArguments {
    fn query(self) -> AdvisoryAuditQuery {
        AdvisoryAuditQuery {
            limit: self.limit,
            scope_id: self.scope_id,
            after: self.after,
            capability: self.capability,
            decision_point: self.decision_point,
            reason: self.reason,
            state: self.state,
        }
    }
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ScopeAuditArguments {
    scope_id: uuid::Uuid,
    limit: u32,
    #[serde(default)]
    after: Option<uuid::Uuid>,
    #[serde(default)]
    capability: Option<AdvisoryCapability>,
    #[serde(default)]
    decision_point: Option<AdvisoryDecisionPoint>,
    #[serde(default)]
    reason: Option<AdvisoryReason>,
    #[serde(default)]
    state: Option<AdvisoryOpportunityState>,
}

impl ScopeAuditArguments {
    fn query(&self) -> AdvisoryAuditQuery {
        AdvisoryAuditQuery {
            limit: self.limit,
            scope_id: None,
            after: self.after,
            capability: self.capability,
            decision_point: self.decision_point,
            reason: self.reason,
            state: self.state,
        }
    }
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ScopeGetArguments {
    scope_id: uuid::Uuid,
    opportunity_id: uuid::Uuid,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum MatrixCardDetail {
    #[default]
    Summary,
    Full,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct MatrixCardArguments {
    task_id: uuid::Uuid,
    expected_task_revision: i64,
    #[serde(default)]
    card_id: Option<String>,
    #[serde(default)]
    detail: MatrixCardDetail,
}

pub(crate) const MATRIX_CARD_IDS: [&str; 5] = [
    "EM02-SCOPE@0.1",
    "EM02-PROTECT@0.1",
    "EM02-OPERATE@0.1",
    "EM02-CAPACITY@0.1",
    "EM02-HOTFIX@0.1",
];

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct CandidateAuditArguments {
    candidate_set_id: uuid::Uuid,
    limit: u32,
    #[serde(default)]
    after: Option<uuid::Uuid>,
    #[serde(default)]
    capability: Option<AdvisoryCapability>,
    #[serde(default)]
    decision_point: Option<AdvisoryDecisionPoint>,
    #[serde(default)]
    reason: Option<AdvisoryReason>,
    #[serde(default)]
    state: Option<AdvisoryOpportunityState>,
}

impl CandidateAuditArguments {
    fn query(&self) -> AdvisoryAuditQuery {
        AdvisoryAuditQuery {
            limit: self.limit,
            scope_id: None,
            after: self.after,
            capability: self.capability,
            decision_point: self.decision_point,
            reason: self.reason,
            state: self.state,
        }
    }
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct CandidateGetArguments {
    candidate_set_id: uuid::Uuid,
    opportunity_id: uuid::Uuid,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct CandidateVerifyArguments {
    request_id: uuid::Uuid,
    opportunity_id: uuid::Uuid,
    candidate_set_id: uuid::Uuid,
    caller_link_id: uuid::Uuid,
    caller_receipt_request_id: uuid::Uuid,
    target_revision: i64,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ScopeAdvisoryRequestArguments {
    request_id: uuid::Uuid,
    candidate_set_id: uuid::Uuid,
    #[serde(default)]
    request_preference: AdvisoryRequestPreference,
    #[serde(default)]
    authored_scope_set: Option<AuthoredScopeSet>,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ScopeAdvisoryDispositionArguments {
    opportunity_id: uuid::Uuid,
    candidate_set_id: uuid::Uuid,
    request_id: uuid::Uuid,
    advice_id: ScopeAdviceId,
    expected_revision: i64,
    action: ScopeDispositionAction,
    selected_id: Option<ScopeAlternativeId>,
    items: Vec<ScopeDispositionItem>,
    rationale: String,
}

pub(crate) enum AdvisoryInvocation {
    MatrixCard {
        task_id: uuid::Uuid,
        expected_task_revision: i64,
        card_id: Option<String>,
        detail: MatrixCardDetail,
    },
    VerifySelectedSave(VerifySelectedSave),
    ScopeRequest(RunScopeAdvisory),
    ScopeDisposition {
        opportunity_id: uuid::Uuid,
        candidate_set_id: uuid::Uuid,
        request: ScopeDispositionRequest,
    },
    Config,
    Configure(ConfigureWorkspaceAdvisory),
    WorkspaceAudit(AdvisoryAuditQuery),
    ScopeAudit {
        scope_id: uuid::Uuid,
        query: AdvisoryAuditQuery,
    },
    ScopeGet {
        scope_id: uuid::Uuid,
        opportunity_id: uuid::Uuid,
    },
    CandidateAudit {
        candidate_set_id: uuid::Uuid,
        query: AdvisoryAuditQuery,
    },
    CandidateGet {
        candidate_set_id: uuid::Uuid,
        opportunity_id: uuid::Uuid,
    },
}

pub(crate) fn parse(name: &str, arguments: Value) -> Result<AdvisoryInvocation> {
    if matches!(
        name,
        "workspace_advisory_audit" | "scope_advisory_audit" | "candidate_advisory_audit"
    ) && [
        "scope_id",
        "candidate_set_id",
        "after",
        "capability",
        "decision_point",
        "reason",
        "state",
    ]
    .iter()
    .any(|field| arguments.get(*field).is_some_and(Value::is_null))
    {
        return Err(Error::InvalidArguments);
    }
    match name {
        "scope_advisory_card" => {
            if ["card_id", "detail"]
                .iter()
                .any(|field| arguments.get(*field).is_some_and(Value::is_null))
            {
                return Err(Error::InvalidArguments);
            }
            let arguments: MatrixCardArguments =
                serde_json::from_value(arguments).map_err(Error::invalid_arguments_from)?;
            if arguments.task_id.is_nil()
                || arguments.expected_task_revision < 1
                || arguments.detail == MatrixCardDetail::Full && arguments.card_id.is_none()
                || arguments
                    .card_id
                    .as_deref()
                    .is_some_and(|id| !MATRIX_CARD_IDS.contains(&id))
            {
                return Err(Error::InvalidArguments);
            }
            Ok(AdvisoryInvocation::MatrixCard {
                task_id: arguments.task_id,
                expected_task_revision: arguments.expected_task_revision,
                card_id: arguments.card_id,
                detail: arguments.detail,
            })
        }
        "candidate_advisory_verify" => {
            let arguments: CandidateVerifyArguments =
                serde_json::from_value(arguments).map_err(Error::invalid_arguments_from)?;
            let request = VerifySelectedSave {
                request_id: arguments.request_id,
                opportunity_id: arguments.opportunity_id,
                candidate_set_id: arguments.candidate_set_id,
                caller_link_id: arguments.caller_link_id,
                caller_receipt_request_id: arguments.caller_receipt_request_id,
                target_revision: arguments.target_revision,
            };
            if request.request_id.is_nil()
                || request.opportunity_id.is_nil()
                || request.candidate_set_id.is_nil()
                || request.caller_link_id.is_nil()
                || request.caller_receipt_request_id.is_nil()
                || request.target_revision < 1
            {
                return Err(Error::InvalidArguments);
            }
            Ok(AdvisoryInvocation::VerifySelectedSave(request))
        }
        "scope_advisory_disposition" => {
            if arguments.get("selected_id").is_some_and(Value::is_null) {
                return Err(Error::InvalidArguments);
            }
            let arguments: ScopeAdvisoryDispositionArguments =
                serde_json::from_value(arguments).map_err(Error::invalid_arguments_from)?;
            if arguments.opportunity_id.is_nil()
                || arguments.candidate_set_id.is_nil()
                || arguments.request_id.is_nil()
                || arguments.expected_revision < 0
            {
                return Err(Error::InvalidArguments);
            }
            arguments.advice_id.validate()?;
            Ok(AdvisoryInvocation::ScopeDisposition {
                opportunity_id: arguments.opportunity_id,
                candidate_set_id: arguments.candidate_set_id,
                request: ScopeDispositionRequest {
                    request_id: arguments.request_id,
                    advice_id: arguments.advice_id,
                    expected_revision: arguments.expected_revision,
                    action: arguments.action,
                    selected_id: arguments.selected_id,
                    items: arguments.items,
                    rationale: arguments.rationale,
                },
            })
        }
        "scope_advisory_request" => {
            if arguments
                .get("authored_scope_set")
                .is_some_and(Value::is_null)
            {
                return Err(Error::InvalidArguments);
            }
            let arguments: ScopeAdvisoryRequestArguments =
                serde_json::from_value(arguments).map_err(Error::invalid_arguments_from)?;
            if arguments.request_id.is_nil() || arguments.candidate_set_id.is_nil() {
                return Err(Error::InvalidArguments);
            }
            if let Some(authored) = &arguments.authored_scope_set {
                authored.validate()?;
            }
            Ok(AdvisoryInvocation::ScopeRequest(RunScopeAdvisory {
                request_id: arguments.request_id,
                candidate_set_id: arguments.candidate_set_id,
                session_preference: AdvisoryRequestPreference::UseWorkspace,
                request_preference: arguments.request_preference,
                authored_scope_set: arguments.authored_scope_set,
            }))
        }
        "get_advisory_config" => {
            if arguments.as_object().is_some_and(|value| value.is_empty()) {
                Ok(AdvisoryInvocation::Config)
            } else {
                Err(Error::InvalidArguments)
            }
        }
        "configure_advisory" => {
            let request: tect_domain::ConfigureWorkspaceAdvisory =
                serde_json::from_value(arguments).map_err(Error::invalid_arguments_from)?;
            request.validate()?;
            Ok(AdvisoryInvocation::Configure(request))
        }
        "workspace_advisory_audit" => {
            let arguments: WorkspaceAuditArguments =
                serde_json::from_value(arguments).map_err(Error::invalid_arguments_from)?;
            let query = arguments.query();
            query.validate()?;
            Ok(AdvisoryInvocation::WorkspaceAudit(query))
        }
        "scope_advisory_audit" => {
            let arguments: ScopeAuditArguments =
                serde_json::from_value(arguments).map_err(Error::invalid_arguments_from)?;
            if arguments.scope_id.is_nil() {
                return Err(Error::InvalidArguments);
            }
            let query = arguments.query();
            query.validate()?;
            Ok(AdvisoryInvocation::ScopeAudit {
                scope_id: arguments.scope_id,
                query,
            })
        }
        "scope_advisory_get" => {
            let arguments: ScopeGetArguments =
                serde_json::from_value(arguments).map_err(Error::invalid_arguments_from)?;
            if arguments.scope_id.is_nil() || arguments.opportunity_id.is_nil() {
                return Err(Error::InvalidArguments);
            }
            Ok(AdvisoryInvocation::ScopeGet {
                scope_id: arguments.scope_id,
                opportunity_id: arguments.opportunity_id,
            })
        }
        "candidate_advisory_audit" => {
            let arguments: CandidateAuditArguments =
                serde_json::from_value(arguments).map_err(Error::invalid_arguments_from)?;
            if arguments.candidate_set_id.is_nil() {
                return Err(Error::InvalidArguments);
            }
            let query = arguments.query();
            query.validate()?;
            Ok(AdvisoryInvocation::CandidateAudit {
                candidate_set_id: arguments.candidate_set_id,
                query,
            })
        }
        "candidate_advisory_get" => {
            let arguments: CandidateGetArguments =
                serde_json::from_value(arguments).map_err(Error::invalid_arguments_from)?;
            if arguments.candidate_set_id.is_nil() || arguments.opportunity_id.is_nil() {
                return Err(Error::InvalidArguments);
            }
            Ok(AdvisoryInvocation::CandidateGet {
                candidate_set_id: arguments.candidate_set_id,
                opportunity_id: arguments.opportunity_id,
            })
        }
        _ => Err(Error::InvalidArguments),
    }
}

#[cfg(test)]
#[path = "advisory_tools/tests.rs"]
mod tests;
