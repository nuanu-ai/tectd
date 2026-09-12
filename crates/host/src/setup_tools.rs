use crate::tools::{annotations, object_schema};
use serde::Deserialize;
use serde_json::{Value, json};
use tect_domain::{Error, MAX_SOURCE_PATH_BYTES, Result, SaveSetup};
use uuid::Uuid;

pub(crate) enum SetupInvocation {
    Inspect {
        task_directory: Option<String>,
    },
    Begin {
        request_id: Uuid,
        input: String,
    },
    Get {
        setup_id: Uuid,
        after_input: Option<i64>,
        limit: u32,
    },
    Save(Box<SaveSetup>),
    Record {
        setup_id: Uuid,
        revision: i64,
        request_id: Uuid,
        input: String,
    },
    Apply {
        setup_id: Uuid,
        revision: i64,
    },
    ReadSkill,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Inspect {
    #[serde(default)]
    task_directory: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Begin {
    request_id: Uuid,
    input: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Get {
    setup_id: Uuid,
    #[serde(default)]
    after_input: Option<i64>,
    #[serde(default = "page_size")]
    limit: u32,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    setup_id: Uuid,
    revision: i64,
    request_id: Uuid,
    input: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Apply {
    setup_id: Uuid,
    revision: i64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Skill {
    name: String,
}

fn page_size() -> u32 {
    25
}
fn decode<T: serde::de::DeserializeOwned>(value: Value) -> Result<T> {
    serde_json::from_value(value).map_err(|_| Error::InvalidArguments)
}
fn reject_null(value: &Value, key: &str) -> Result<()> {
    if value.get(key).is_some_and(Value::is_null) {
        Err(Error::InvalidArguments)
    } else {
        Ok(())
    }
}

pub(crate) fn parse(name: &str, arguments: Value) -> Result<SetupInvocation> {
    Ok(match name {
        "inspect_setup" => {
            reject_null(&arguments, "task_directory")?;
            let args: Inspect = decode(arguments)?;
            SetupInvocation::Inspect {
                task_directory: args.task_directory,
            }
        }
        "begin_setup" => {
            let args: Begin = decode(arguments)?;
            SetupInvocation::Begin {
                request_id: args.request_id,
                input: args.input,
            }
        }
        "get_setup" => {
            reject_null(&arguments, "after_input")?;
            let args: Get = decode(arguments)?;
            SetupInvocation::Get {
                setup_id: args.setup_id,
                after_input: args.after_input,
                limit: args.limit,
            }
        }
        "save_setup" => SetupInvocation::Save(Box::new(decode(arguments)?)),
        "record_setup_input" => {
            let args: Record = decode(arguments)?;
            SetupInvocation::Record {
                setup_id: args.setup_id,
                revision: args.revision,
                request_id: args.request_id,
                input: args.input,
            }
        }
        "apply_setup" => {
            let args: Apply = decode(arguments)?;
            SetupInvocation::Apply {
                setup_id: args.setup_id,
                revision: args.revision,
            }
        }
        "read_skill" => {
            let args: Skill = decode(arguments)?;
            if args.name != "tectd-setup" {
                return Err(Error::InvalidArguments);
            }
            SetupInvocation::ReadSkill
        }
        _ => return Err(Error::InvalidArguments),
    })
}

pub(crate) fn definitions() -> Vec<Value> {
    let uuid = json!({"type":"string","format":"uuid"});
    let revision = json!({"type":"integer","minimum":1});
    let text = json!({"type":"string","minLength":1});
    let nullable = json!({"type":["string","null"]});
    vec![
        tool(
            "inspect_setup",
            "Inspect AGENTS.md in the actual launch directory from the current Codex task context. Omission reports context_unknown; do not ask the human to choose a folder.",
            json!({"task_directory":{"type":"string","minLength":1,"maxLength":MAX_SOURCE_PATH_BYTES}}),
            json!([]),
            false,
            true,
        ),
        tool(
            "begin_setup",
            "Persist the complete company/work narrative for the verified missing AGENTS.md in this session's bound task directory. Reuse request_id and exact text on retry.",
            json!({"request_id":uuid,"input":text}),
            json!(["request_id", "input"]),
            false,
            true,
        ),
        tool(
            "get_setup",
            "Read the whole saved draft, notes, pending question and a page of exact original inputs, plus current file observation. Omitted after_input uses the consumed cursor; recovery starts at 0.",
            json!({"setup_id":uuid,"after_input":{"type":"integer","minimum":0},"limit":{"type":"integer","minimum":1,"maximum":100,"default":25}}),
            json!(["setup_id"]),
            true,
            true,
        ),
        tool(
            "save_setup",
            "Persist a revision-checked complete draft or partial work before asking a necessary question. Omission preserves; null clears. ready is required; true requires all inputs incorporated, meaningful content and no pending question.",
            json!({"setup_id":uuid,"revision":revision,"input_cursor":{"type":"integer","minimum":0},"ready":{"type":"boolean"},"content":nullable,"working_notes":nullable,"pending_question":nullable}),
            json!(["setup_id", "revision", "input_cursor", "ready"]),
            false,
            false,
        ),
        tool(
            "record_setup_input",
            "Persist the complete original reply and resume the same setup. Exact retries retain request_id and original text, including after a lost response.",
            json!({"setup_id":uuid,"revision":revision,"request_id":uuid,"input":text}),
            json!(["setup_id", "revision", "request_id", "input"]),
            false,
            true,
        ),
        tool(
            "apply_setup",
            "Create the fixed AGENTS.md exclusively from the durable ready draft and verify its exact bytes. Never overwrite an existing different file. Retry the same setup_id and ready revision after an uncertain result.",
            json!({"setup_id":uuid,"revision":revision}),
            json!(["setup_id", "revision"]),
            false,
            true,
        ),
    ]
}
fn tool(
    name: &str,
    description: &str,
    properties: Value,
    required: Value,
    read: bool,
    idempotent: bool,
) -> Value {
    let mut hints = annotations(read);
    hints["idempotentHint"] = json!(idempotent);
    json!({"name":name,"description":description,"inputSchema":object_schema(properties, required),"annotations":hints})
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn schema_does_not_accept_forged_authority_or_ambiguous_nulls() {
        let id = Uuid::new_v4();
        for (tool, args) in [
            ("inspect_setup", json!({"task_directory":null})),
            ("inspect_setup", json!({"task_directory":"/x","host_id":id})),
            (
                "begin_setup",
                json!({"request_id":id,"input":"x","task_directory":"/x"}),
            ),
            ("get_setup", json!({"setup_id":id,"after_input":null})),
            (
                "save_setup",
                json!({"setup_id":id,"revision":1,"input_cursor":0}),
            ),
            (
                "record_setup_input",
                json!({"setup_id":id,"request_id":id,"input":"x"}),
            ),
            (
                "apply_setup",
                json!({"setup_id":id,"revision":1,"path":"/__tect_test__/AGENTS.md"}),
            ),
        ] {
            assert!(parse(tool, args).is_err(), "{tool}");
        }
        assert!(parse("inspect_setup", json!({})).is_ok());
        assert!(parse("read_skill", json!({"name":"../tectd-setup"})).is_err());
    }
}
