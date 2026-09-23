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

    #[test]
    fn disposition_route_accepts_only_explicit_stored_identity_shape() {
        let id = uuid::Uuid::new_v4();
        let digest = "a".repeat(64);
        let alternative = "b".repeat(64);
        let request = json!({
            "opportunity_id": id,
            "candidate_set_id": id,
            "request_id": id,
            "advice_id": digest,
            "expected_revision": 0,
            "action": "reject_all",
            "items": [{"alternative_id": alternative, "state": "not_selected"}],
            "rationale": "reject this advice"
        });
        let Ok(AdvisoryInvocation::ScopeDisposition {
            request: parsed, ..
        }) = parse("scope_advisory_disposition", request.clone())
        else {
            panic!("valid disposition rejected")
        };
        assert_eq!(parsed.action, ScopeDispositionAction::RejectAll);
        for (field, value) in [
            ("actor_id", json!(id)),
            ("session_id", json!(id)),
            ("workspace_id", json!(id)),
            ("provider_instruction", json!("run")),
        ] {
            let mut changed = request.clone();
            changed[field] = value;
            assert!(parse("scope_advisory_disposition", changed).is_err());
        }
        for (field, value) in [
            ("opportunity_id", json!(uuid::Uuid::nil())),
            ("candidate_set_id", json!(uuid::Uuid::nil())),
            ("request_id", json!(uuid::Uuid::nil())),
            ("expected_revision", json!(-1)),
            ("action", json!("unknown")),
            ("advice_id", json!("unknown")),
        ] {
            let mut changed = request.clone();
            changed[field] = value;
            assert!(parse("scope_advisory_disposition", changed).is_err());
        }
        let mut superseded = request;
        superseded["action"] = json!("supersede_with_deterministic_choice");
        superseded["selected_id"] = json!(alternative);
        superseded["items"][0]["state"] = json!("selected");
        let Ok(AdvisoryInvocation::ScopeDisposition {
            request: parsed, ..
        }) = parse("scope_advisory_disposition", superseded)
        else {
            panic!("valid deterministic supersession rejected")
        };
        assert_eq!(
            parsed.action,
            ScopeDispositionAction::SupersedeWithDeterministicChoice
        );
        assert_eq!(parsed.selected_id.unwrap().0, alternative);
    }

    #[test]
    fn scope_request_binds_session_preference_and_refuses_forged_identity() {
        let request_id = uuid::Uuid::new_v4();
        let candidate_set_id = uuid::Uuid::new_v4();
        let source_ref = uuid::Uuid::new_v4();
        let active = json!({
            "request_id":request_id,
            "candidate_set_id":candidate_set_id,
            "request_preference":"use_workspace",
            "authored_scope_set":{
                "expected_candidate_set_revision":1,
                "baseline_key":"baseline",
                "alternatives":[{
                    "key":"baseline",
                    "kind":"cohesive",
                    "draft":{
                        "boundary":"ongoing", "goals":[], "candidates":[],
                        "empty_disposition":{"kind":"out_of_boundary","reason":"none","source_ref_id":source_ref}
                    },
                    "covered_source_ref_ids":[source_ref]
                }]
            }
        });
        let Ok(AdvisoryInvocation::ScopeRequest(request)) =
            parse("scope_advisory_request", active.clone())
        else {
            panic!("valid authored request was rejected")
        };
        assert_eq!(
            request.session_preference,
            AdvisoryRequestPreference::UseWorkspace
        );
        assert_eq!(
            request.request_preference,
            AdvisoryRequestPreference::UseWorkspace
        );
        assert!(request.authored_scope_set.is_some());
        let skipped = json!({"request_id":request_id,"candidate_set_id":candidate_set_id,"request_preference":"skip"});
        let Ok(AdvisoryInvocation::ScopeRequest(request)) =
            parse("scope_advisory_request", skipped)
        else {
            panic!("skipped request without source material was rejected")
        };
        assert_eq!(request.request_preference, AdvisoryRequestPreference::Skip);
        assert!(request.authored_scope_set.is_none());
        for field in [
            "tenant_id",
            "actor_id",
            "workspace_id",
            "session_id",
            "session_preference",
            "provider_api_key",
        ] {
            let mut forged = active.clone();
            forged[field] = json!(uuid::Uuid::new_v4());
            assert!(
                parse("scope_advisory_request", forged).is_err(),
                "accepted {field}"
            );
        }
        let mut too_many = active.clone();
        let alternative = too_many["authored_scope_set"]["alternatives"][0].clone();
        too_many["authored_scope_set"]["alternatives"] =
            serde_json::Value::Array(vec![alternative; 101]);
        assert!(parse("scope_advisory_request", too_many).is_err());
        for malformed in [
            json!({"request_id":uuid::Uuid::nil(),"candidate_set_id":candidate_set_id}),
            json!({"request_id":request_id,"candidate_set_id":candidate_set_id,"authored_scope_set":null}),
        ] {
            assert!(parse("scope_advisory_request", malformed).is_err());
        }
    }
}
