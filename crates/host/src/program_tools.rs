use crate::tools::{annotations, object_schema};
use serde::Deserialize;
use serde_json::{Value, json};
use tect_domain::{Error, ProgramCursor, Result, SaveProgram};
use uuid::Uuid;

pub(crate) enum ProgramInvocation {
    Begin {
        request_id: Uuid,
        input: String,
    },
    Get {
        program_id: Uuid,
        after_input: Option<i64>,
        limit: u32,
    },
    Save(Box<SaveProgram>),
    Record {
        program_id: Uuid,
        request_id: Uuid,
        input: String,
    },
    List {
        after: Option<ProgramCursor>,
        limit: u32,
    },
    ReadSkill,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BeginArguments {
    request_id: Uuid,
    input: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RecordArguments {
    program_id: Uuid,
    request_id: Uuid,
    input: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GetArguments {
    program_id: Uuid,
    #[serde(default)]
    after_input: Option<i64>,
    #[serde(default = "page_size")]
    limit: u32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ListArguments {
    #[serde(default)]
    after: Option<String>,
    #[serde(default = "page_size")]
    limit: u32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SkillArguments {
    name: String,
}

fn page_size() -> u32 {
    25
}

fn decode<T: serde::de::DeserializeOwned>(value: Value) -> Result<T> {
    serde_json::from_value(value).map_err(|_| Error::InvalidArguments)
}

pub(crate) fn parse(name: &str, arguments: Value) -> Result<ProgramInvocation> {
    match name {
        "begin_program" => {
            let args: BeginArguments = decode(arguments)?;
            Ok(ProgramInvocation::Begin {
                request_id: args.request_id,
                input: args.input,
            })
        }
        "get_program" => {
            reject_null(&arguments, "after_input")?;
            let args: GetArguments = decode(arguments)?;
            Ok(ProgramInvocation::Get {
                program_id: args.program_id,
                after_input: args.after_input,
                limit: args.limit,
            })
        }
        "save_program" => Ok(ProgramInvocation::Save(Box::new(decode(arguments)?))),
        "record_program_input" => {
            let args: RecordArguments = decode(arguments)?;
            Ok(ProgramInvocation::Record {
                program_id: args.program_id,
                request_id: args.request_id,
                input: args.input,
            })
        }
        "list_programs" => {
            reject_null(&arguments, "after")?;
            let args: ListArguments = decode(arguments)?;
            let after = args
                .after
                .as_deref()
                .map(ProgramCursor::parse)
                .transpose()?;
            Ok(ProgramInvocation::List {
                after,
                limit: args.limit,
            })
        }
        "read_skill" => {
            let args: SkillArguments = decode(arguments)?;
            if args.name != "tectd-program" {
                return Err(Error::InvalidArguments);
            }
            Ok(ProgramInvocation::ReadSkill)
        }
        _ => Err(Error::InvalidArguments),
    }
}

fn reject_null(arguments: &Value, field: &str) -> Result<()> {
    if arguments.get(field).is_some_and(Value::is_null) {
        return Err(Error::InvalidArguments);
    }
    Ok(())
}

pub(crate) fn definitions() -> Vec<Value> {
    let uuid = json!({"type":"string","format":"uuid"});
    let text = json!({"type":"string","minLength":1});
    let nullable = json!({"type":["string","null"]});
    let limit = json!({"type":"integer","minimum":1,"maximum":100,"default":25});
    let input = json!({"request_id":uuid,"input":text});
    vec![
        tool(
            "begin_program",
            "Persist one original narrative and a resumable draft. Reuse request_id only for an exact retry.",
            input,
            json!(["request_id", "input"]),
            false,
            true,
        ),
        tool(
            "get_program",
            "Read current PRD and a page of original input. Omitted after_input starts at the saved coverage cursor.",
            json!({"program_id":uuid,"after_input":{"type":"integer","minimum":0},"limit":limit}),
            json!(["program_id"]),
            true,
            true,
        ),
        tool(
            "save_program",
            "Save a revision-checked patch. Omission preserves; null clears. Complete opens the same coherent Program without launching work.",
            json!({
                "program_id":uuid,"revision":{"type":"integer","minimum":1},
                "input_cursor":{"type":"integer","minimum":0},
                "name":nullable,"intent":nullable,"basis":nullable,"boundaries":nullable,
                "constraints":nullable,"success":nullable,"working_notes":nullable,
                "pending_question":nullable,"complete":{"type":"boolean","default":false}
            }),
            json!(["program_id", "revision", "input_cursor"]),
            false,
            false,
        ),
        tool(
            "record_program_input",
            "Append the human's original reply or correction and resume the same Program. Exact retries retain request_id.",
            json!({"program_id":uuid,"request_id":uuid,"input":text}),
            json!(["program_id", "request_id", "input"]),
            false,
            true,
        ),
        tool(
            "list_programs",
            "Page through this workspace's Programs, unfinished first. Use the exact returned cursor for the next page.",
            json!({"after":{"type":"string","pattern":"^[wr]:[0-9a-fA-F-]+$"},"limit":limit}),
            json!([]),
            true,
            true,
        ),
        tool(
            "read_skill",
            "Read the packaged method named by the current Program or workspace setup step.",
            json!({"name":{"type":"string","enum":["tectd-program","tectd-setup"]}}),
            json!(["name"]),
            true,
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
    json!({"name":name,"description":description,
        "inputSchema":object_schema(properties, required),"annotations":hints})
}

#[cfg(test)]
mod tests {
    use super::*;
    use tect_domain::TextPatch;

    #[test]
    fn nullable_patch_does_not_treat_missing_as_null() {
        let base = json!({"program_id":Uuid::new_v4(),"revision":1,"input_cursor":0});
        let ProgramInvocation::Save(omitted) = parse("save_program", base.clone()).unwrap() else {
            panic!()
        };
        assert_eq!(omitted.name, TextPatch::Unchanged);
        let mut cleared = base;
        cleared["name"] = Value::Null;
        let ProgramInvocation::Save(cleared) = parse("save_program", cleared).unwrap() else {
            panic!()
        };
        assert_eq!(cleared.name, TextPatch::Set(None));
    }

    #[test]
    fn business_schema_refuses_forged_authority_and_skill_paths() {
        let mut base = json!({"program_id":Uuid::new_v4(),"revision":1,"input_cursor":0});
        for field in ["status", "current_step", "workspace_id", "description"] {
            base[field] = json!("forged");
            assert!(parse("save_program", base.clone()).is_err());
            base.as_object_mut().unwrap().remove(field);
        }
        for name in ["../SKILL.md", "/etc/passwd", "tectd-program/../other"] {
            assert!(parse("read_skill", json!({"name":name})).is_err());
        }
        assert!(parse("list_programs", json!({"after":null})).is_err());
        assert!(
            parse(
                "get_program",
                json!({"program_id":Uuid::new_v4(),"limit":1.5})
            )
            .is_err()
        );
    }
}
