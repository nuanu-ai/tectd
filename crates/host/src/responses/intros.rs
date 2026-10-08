use super::*;

pub(super) fn intro(data: &Value) -> &'static str {
    if data["name"] == "tectd-setup" && data.get("body").is_some() {
        return "Use this method with the current workspace setup calls.";
    }
    if let Some(step) = data["setup"]["current_step"].as_str() {
        return match step {
            "compose" => {
                "Setup is saved. Read its method and original input, then show and save the whole proposed AGENTS.md."
            }
            "waiting_input" => {
                "The setup draft and pending question are saved. Ask only that unresolved question, then record the complete original reply."
            }
            "ready_to_apply" => {
                "The complete setup draft is durable and ready. Show the whole file and use the exact apply call."
            }
            "complete"
                if data["file"]["observed_now"] == true
                    && (data["file"]["status"] == "missing"
                        || data["file"]["sha256"].is_string()
                            && data["file"]["sha256"] != data["setup"]["applied_sha256"]) =>
            {
                "Setup was applied previously, but the current file is missing or differs from the applied content. This conflict is preserved; no file was recreated or overwritten."
            }
            "complete" if data["file"]["observed_now"] == true => {
                "Setup is historically applied; the current file observation and whole saved content are below. Only matching current bytes verify that result now."
            }
            "complete" => {
                "Setup is historically applied. This call did not verify the file now; the whole saved content is below."
            }
            _ => INTROS[7],
        };
    }
    if data["name"] == "tectd-program" && data.get("body").is_some() {
        return INTROS[6];
    }
    if data["operation"] == "begin" && data["program"]["status"] == "draft" {
        return "A Program Draft and its original narrative are recorded. Read the save contract and original inputs before composing the PRD.";
    }
    if let Some(step) = data["program"]["current_step"].as_str() {
        return match step {
            "compose" => INTROS[3],
            "waiting_input" => INTROS[4],
            "ready" => INTROS[5],
            _ => INTROS[7],
        };
    }
    if data["status"] == "uninitialized" {
        return INTROS[0];
    }
    if data["programs_delivery"] == "use_list_programs" {
        return "The Program listing is available through the exact query route program.list call from the beginning. Full names did not fit beside this workspace context; the current file observation is included below.";
    }
    if data.get("file").is_some() {
        return match data["file"]["status"].as_str() {
            Some("missing") => {
                "AGENTS.md is currently absent in this task directory. Continue its saved setup or compose it from the existing company/work narrative. Programs remain available."
            }
            Some("existing") => {
                "AGENTS.md already exists and is preserved. Current setup context and available Programs are below."
            }
            Some("unavailable") => {
                "AGENTS.md could not be safely inspected with current access. This does not establish absence. Programs remain available."
            }
            _ if data["setup_context"]["setup"].is_object() => {
                "A saved setup is available. Use its exact query route setup.get call to restore the draft and original history and inspect the current file. Programs remain available."
            }
            _ => {
                "This is saved workspace state; the file's current presence is unknown. The exact inspection action accepts the known task launch directory. Programs remain available."
            }
        };
    }
    if let Some(programs) = data["programs"].as_array() {
        return if programs.is_empty() {
            INTROS[1]
        } else {
            INTROS[2]
        };
    }
    INTROS[7]
}

pub(crate) fn error_intro(error: &Error) -> &'static str {
    if error
        .refusal()
        .is_some_and(|refusal| refusal.code == tect_domain::RefusalCode::StateConflict)
    {
        return "The requested transition conflicts with saved state. Follow the exact owner context action and preserve completed work when adding a successor.";
    }
    if error
        .refusal()
        .is_some_and(|refusal| refusal.code == tect_domain::RefusalCode::InvalidOutput)
    {
        return "The submitted output does not satisfy the required contract. Correct the reported issue and retry.";
    }
    match error.pipeline_source() {
        Error::StaleRevision => {
            "A newer revision exists. Reload the saved record and merge before saving."
        }
        Error::StaleContext | Error::ContextChanged => {
            "The saved context changed. Reload its exact owner context and follow the supplied recovery action."
        }
        Error::NeedsContext => {
            "The current operation has unresolved context needs. Reload its exact owner context and follow the supplied recovery action."
        }
        Error::KnowledgeUnavailable => {
            "Durable knowledge is unavailable because its capability, database identity, or integrity check is not ready. Follow the supplied context or operator recovery action."
        }
        Error::KnowledgeLifecycleRequired => {
            "This durable change is owned by the Knowledge Change lifecycle. Continue with its exact owner and source-bound begin contract."
        }
        Error::CapacityExceeded => {
            "The complete required durable knowledge context exceeds the bounded transport capacity. No partial context was returned."
        }
        Error::InputPending => {
            "The input cursor is not current. Read the remaining original input before completing."
        }
        Error::ProgramIncomplete => {
            "Keep every required PRD field meaningful and resolve the pending question before completion."
        }
        Error::SetupIncomplete => {
            "The setup needs coherent content, every input incorporated and no pending question before execute route setup.apply can proceed."
        }
        Error::SetupFileConflict => {
            "The current AGENTS.md is missing after prior application, changed, or conflicts with the intended content. It was not overwritten or recreated. Reload the current observation."
        }
        Error::SetupAlreadyApplied => {
            "This initial setup is already applied and cannot be edited through setup. Reload its saved content and current file observation."
        }
        Error::TaskDirectoryUnbound => {
            "Supply the launch directory already known from this task context through the exact inspection action; do not ask the human to choose a folder."
        }
        Error::SetupExists => {
            "A setup already exists for this task directory. Read workspace state and resume that same setup."
        }
        Error::InputConflict => {
            "This request identity already belongs to different original input. Reload; use a new request identity for a new input."
        }
        Error::WorkspaceNotOpen => "Open this native session's workspace before continuing.",
        Error::RequestTooLarge => {
            "The encoded request or response exceeds transport capacity. Reload current state before retrying."
        }
        Error::OperationTimeout => {
            "The daemon reached its operation deadline. The result is uncertain; inspect saved state before further writes."
        }
        Error::StorageUnavailable | Error::TransportUnavailable => {
            "The result is uncertain. Recover with the exact call below; do not create a replacement record."
        }
        Error::InvalidArguments => {
            "The arguments do not match the selected route schema. Correct the input and retry."
        }
        Error::InvalidArgumentsDetail(_) => {
            "The arguments do not match the selected route schema. Correct the input using the reported diagnostic."
        }
        Error::InvalidPipelineArtifact(_) => {
            "A pipeline artifact failed its phase contract. Correct every reported violation and retry the supplied phase action."
        }
        Error::InternalInvariant => {
            "TectD could not construct a valid next call. No follow-up action was emitted."
        }
        Error::Refused(refusal) if refusal.code != tect_domain::RefusalCode::AuthorityRequired => {
            "The request was refused. Follow the reported rule and next action."
        }
        _ => {
            "The request cannot proceed with the current identity or access. No protected data is included."
        }
    }
}

pub(super) fn failure_intro(error: &Error, call: Option<(&str, &Value)>) -> &'static str {
    if !matches!(error.pipeline_source(), Error::OperationTimeout) {
        return error_intro(error);
    }
    match call {
        Some((name, _)) if crate::api::read_only_internal_call(name) => {
            "The daemon reached its operation deadline while reading. Retry the read if needed."
        }
        Some((_, arguments))
            if arguments
                .pointer("/params/request_id")
                .or_else(|| arguments.get("request_id"))
                .and_then(Value::as_str)
                .is_some() =>
        {
            "The daemon reached its operation deadline. Read saved state when available; if the result is still absent, retry only the exact same request ID and payload. Do not create a replacement record."
        }
        _ => error_intro(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wrap(error: Error, code: tect_domain::RefusalCode) -> Error {
        Error::PipelineRefused {
            source: Box::new(error),
            refusal: Box::new(
                tect_domain::Refusal::new(code)
                    .with_rule("outer-rule")
                    .with_path("outer/path"),
            ),
        }
    }

    #[test]
    fn refusal_classification_and_diagnostics_survive_real_failure_serialization() {
        use tect_domain::{PipelineArtifactDiagnostic, PipelineArtifactViolation, RefusalCode};
        let output_intro = "The submitted output does not satisfy the required contract. Correct the reported issue and retry.";
        let schema_intro =
            "The arguments do not match the selected route schema. Correct the input and retry.";
        let diagnostic_intro = "The arguments do not match the selected route schema. Correct the input using the reported diagnostic.";
        let refused_intro = "The request was refused. Follow the reported rule and next action.";
        let access_intro = "The request cannot proceed with the current identity or access. No protected data is included.";
        let artifact =
            Error::InvalidPipelineArtifact(Box::new(PipelineArtifactDiagnostic::bounded(
                "invalid_artifact".into(),
                "review".into(),
                "review.json".into(),
                vec![PipelineArtifactViolation {
                    code: "missing".into(),
                    path: "/decision".into(),
                    expected: Some("ready".into()),
                    actual: None,
                }],
                true,
                "correct_output".into(),
            )));
        let cases = [
            (
                Error::refused(RefusalCode::InvalidOutput, "correct_output", "output"),
                output_intro,
            ),
            (artifact, output_intro),
            (Error::InvalidArguments, schema_intro),
            (
                Error::invalid_arguments_at("missing field `decision`", "/output/decision"),
                diagnostic_intro,
            ),
            (
                Error::refused(RefusalCode::EvidenceMissing, "supply_evidence", "evidence"),
                refused_intro,
            ),
            (
                Error::refused(
                    RefusalCode::AuthorityRequired,
                    "obtain_authority",
                    "authority",
                ),
                access_intro,
            ),
            (Error::Unauthorized, access_intro),
        ];
        for (mut error, expected_intro) in cases {
            let expected_details = error
                .argument_diagnostic()
                .map(|d| serde_json::to_value(d).unwrap())
                .or_else(|| {
                    error
                        .pipeline_artifact_diagnostic()
                        .map(|d| serde_json::to_value(d).unwrap())
                });
            let code = error
                .refusal()
                .map_or(RefusalCode::AuthorityRequired, |r| r.code);
            for depth in 0..3 {
                let expected_refusal = serde_json::to_value(error.refusal().unwrap()).unwrap();
                let response = failure(error.clone(), None);
                assert_eq!(response["isError"], true);
                assert_eq!(response["content"][0]["text"], expected_intro);
                let body: Value =
                    serde_json::from_str(response["content"][1]["text"].as_str().unwrap()).unwrap();
                assert_eq!(body["error"]["code"], error.code());
                if super::super::access_denial(&error) {
                    assert_eq!(body["error"]["refusal"]["code"], expected_refusal["code"]);
                    assert!(body["error"]["refusal"].get("path").is_none());
                } else {
                    assert_eq!(body["error"]["refusal"], expected_refusal);
                }
                assert_eq!(body["error"].get("details"), expected_details.as_ref());
                if depth > 0 && !super::super::access_denial(&error) {
                    assert_eq!(body["error"]["refusal"]["rule"], "outer-rule");
                    assert_eq!(body["error"]["refusal"]["path"], "outer/path");
                }
                error = wrap(error, code);
            }
        }
        // An explicit outer output refusal governs classification even for a schema source.
        let response = failure(
            wrap(Error::InvalidArguments, RefusalCode::InvalidOutput),
            None,
        );
        assert_eq!(response["content"][0]["text"], output_intro);
    }

    #[test]
    fn internal_read_timeout_guidance_matches_catalogue() {
        for route in [
            "knowledge_search",
            "knowledge_lifecycle",
            "list_programs",
            "slice_pipelines",
        ] {
            let args = json!({});
            let intro = failure_intro(&Error::OperationTimeout, Some((route, &args)));
            assert!(intro.contains("Retry the read"), "{route}");
            assert!(!intro.contains("request ID"), "{route}");
        }
        let search = failure(
            Error::OperationTimeout,
            Some((
                "knowledge_search",
                &json!({"mode":"lexical","query":"current evidence","purpose":"lookup"}),
            )),
        );
        assert!(
            search["content"][0]["text"]
                .as_str()
                .unwrap()
                .contains("Retry the read")
        );
        let write = failure(
            Error::OperationTimeout,
            Some(("open_workspace", &json!({}))),
        );
        assert!(
            write["content"][0]["text"]
                .as_str()
                .unwrap()
                .contains("result is uncertain")
        );
    }
}
