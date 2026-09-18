use serde::Deserialize;
use serde_json::Value;
use tect_domain::{
    Error, PlanningTaskContext, ProgramCursor, RefreshProgramKnowledge, Result, SaveProgram,
};
use uuid::Uuid;

pub(crate) enum ProgramInvocation {
    Begin {
        request_id: Uuid,
        input: String,
        task_context: PlanningTaskContext,
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
        task_context: Option<PlanningTaskContext>,
    },
    Refresh(Box<RefreshProgramKnowledge>),
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
    #[serde(default)]
    task_context: PlanningTaskContext,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RecordArguments {
    program_id: Uuid,
    request_id: Uuid,
    input: String,
    #[serde(default)]
    task_context: Option<PlanningTaskContext>,
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
    serde_json::from_value(value).map_err(Error::invalid_arguments_from)
}

pub(crate) fn parse(name: &str, arguments: Value) -> Result<ProgramInvocation> {
    match name {
        "begin_program" => {
            let args: BeginArguments = decode(arguments)?;
            Ok(ProgramInvocation::Begin {
                request_id: args.request_id,
                input: args.input,
                task_context: args.task_context,
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
        "save_program" => {
            reject_null(&arguments, "consumed_knowledge")?;
            Ok(ProgramInvocation::Save(Box::new(decode(arguments)?)))
        }
        "record_program_input" => {
            reject_null(&arguments, "task_context")?;
            let args: RecordArguments = decode(arguments)?;
            Ok(ProgramInvocation::Record {
                program_id: args.program_id,
                request_id: args.request_id,
                input: args.input,
                task_context: args.task_context,
            })
        }
        "refresh_program_knowledge" => {
            reject_null(&arguments, "task_context")?;
            Ok(ProgramInvocation::Refresh(Box::new(decode(arguments)?)))
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
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
        assert!(
            parse(
                "save_program",
                json!({"program_id":Uuid::new_v4(),"revision":1,"input_cursor":0,"consumed_knowledge":null})
            )
            .is_err()
        );
        assert!(
            parse(
                "record_program_input",
                json!({"program_id":Uuid::new_v4(),"request_id":Uuid::new_v4(),"input":"x","task_context":null})
            )
            .is_err()
        );
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
