use serde_json::Value;
use tect_domain::{
    AdvisoryAuditQuery, AdvisoryCapability, AdvisoryDecisionPoint, AdvisoryOpportunityState,
    AdvisoryReason, ConfigureWorkspaceAdvisory, Error, Result,
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

pub(crate) enum AdvisoryInvocation {
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
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn advisory_routes_decode_only_their_strict_shapes() {
        assert!(matches!(
            parse("get_advisory_config", json!({})),
            Ok(AdvisoryInvocation::Config)
        ));
        assert!(matches!(
            parse(
                "configure_advisory",
                json!({
                    "expected_revision": 0,
                    "mode": "optional",
                    "provider_profile_ref": {"id": "jev-production"},
                    "model_configuration": {"model": "jev-advisory-v1"}
                })
            ),
            Ok(AdvisoryInvocation::Configure(_))
        ));
        let scope_id = uuid::Uuid::new_v4();
        let opportunity_id = uuid::Uuid::new_v4();
        assert!(matches!(
            parse(
                "workspace_advisory_audit",
                json!({"limit": 50, "scope_id": scope_id})
            ),
            Ok(AdvisoryInvocation::WorkspaceAudit(_))
        ));
        assert!(matches!(
            parse(
                "scope_advisory_audit",
                json!({
                    "scope_id": scope_id,
                    "limit": 25,
                    "capability": "scope_decomposition",
                    "decision_point": "scope.decomposition.before_selection",
                    "reason": "request_skip",
                    "state": "no_call"
                })
            ),
            Ok(AdvisoryInvocation::ScopeAudit { .. })
        ));
        assert!(matches!(
            parse(
                "scope_advisory_get",
                json!({"scope_id":scope_id,"opportunity_id":opportunity_id})
            ),
            Ok(AdvisoryInvocation::ScopeGet { .. })
        ));
        assert!(matches!(
            parse(
                "candidate_advisory_audit",
                json!({"candidate_set_id":scope_id,"limit":25})
            ),
            Ok(AdvisoryInvocation::CandidateAudit { .. })
        ));
        assert!(matches!(
            parse(
                "candidate_advisory_get",
                json!({"candidate_set_id":scope_id,"opportunity_id":opportunity_id})
            ),
            Ok(AdvisoryInvocation::CandidateGet { .. })
        ));

        for (name, arguments) in [
            ("get_advisory_config", json!({"forged": true})),
            (
                "configure_advisory",
                json!({"expected_revision": 0, "mode": "optional", "forged": true}),
            ),
            (
                "configure_advisory",
                json!({"expected_revision": 0, "mode": null}),
            ),
            (
                "configure_advisory",
                json!({"expected_revision": 0, "mode": "optional", "provider_api_key": "secret"}),
            ),
            (
                "configure_advisory",
                json!({
                    "expected_revision": 0,
                    "mode": "optional",
                    "provider_profile_ref": {"id": "jev-production"}
                }),
            ),
            (
                "configure_advisory",
                json!({
                    "expected_revision": 0,
                    "mode": "optional",
                    "model_configuration": {"model": "jev-advisory-v1"}
                }),
            ),
            ("workspace_advisory_audit", json!({"limit": 0})),
            (
                "workspace_advisory_audit",
                json!({"limit": 10, "scope_id": uuid::Uuid::nil()}),
            ),
            (
                "workspace_advisory_audit",
                json!({"limit": 10, "after": null}),
            ),
            (
                "scope_advisory_audit",
                json!({"scope_id":scope_id,"limit":10,"forged":true}),
            ),
            (
                "scope_advisory_audit",
                json!({
                    "scope_id":scope_id,
                    "limit":10,
                    "capability":"model_routing",
                    "decision_point":"scope.decomposition.before_selection"
                }),
            ),
            (
                "scope_advisory_get",
                json!({"scope_id":scope_id,"opportunity_id":uuid::Uuid::nil()}),
            ),
            (
                "candidate_advisory_get",
                json!({"candidate_set_id":scope_id,"opportunity_id":opportunity_id,"scope_id":scope_id}),
            ),
            (
                "candidate_advisory_get",
                json!({"candidate_set_id":uuid::Uuid::nil(),"opportunity_id":opportunity_id}),
            ),
            (
                "candidate_advisory_audit",
                json!({"candidate_set_id":scope_id,"limit":10,"scope_id":scope_id}),
            ),
            (
                "candidate_advisory_audit",
                json!({"candidate_set_id":scope_id,"limit":10,"after":null}),
            ),
            (
                "candidate_advisory_audit",
                json!({"candidate_set_id":scope_id,"limit":0}),
            ),
        ] {
            assert!(parse(name, arguments).is_err(), "accepted forged {name}");
        }
    }
}
