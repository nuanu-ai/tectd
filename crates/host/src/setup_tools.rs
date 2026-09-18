use serde::Deserialize;
use serde_json::Value;
use tect_domain::{Error, Result, SaveSetup};
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
    serde_json::from_value(value).map_err(Error::invalid_arguments_from)
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
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
