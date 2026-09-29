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
            "Supply the launch directory already known from this Codex task context through the exact inspection action; do not ask the human to choose a folder."
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
            "The arguments do not match the selected route schema. Correct the input using the returned route contract and retry."
        }
        Error::InvalidArgumentsDetail(_) => {
            "The arguments do not match the selected route schema. The response includes the violation, pointer, and complete route contract for a direct retry."
        }
        Error::InvalidPipelineArtifact(_) => {
            "A pipeline artifact failed its phase contract. Correct every reported violation and retry the supplied phase action."
        }
        Error::InternalInvariant => {
            "TectD could not construct a valid next call. No follow-up action was emitted."
        }
        _ => {
            "The request cannot proceed with the current identity or access. No protected data is included."
        }
    }
}
