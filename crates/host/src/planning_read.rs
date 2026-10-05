//! Byte windows for authorized planning reads; selectors stay on the original read route.
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tect_domain::{Error, RefusalCode, Result};

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Window {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset_bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit_bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub representation_digest: Option<String>,
}
impl Window {
    pub(crate) fn validate(&self) -> Result<()> {
        tect_domain::validate_pipeline_fragment(
            self.offset_bytes,
            self.limit_bytes,
            self.representation_digest.as_deref(),
        )
    }
    pub(crate) fn borrowed(&self) -> crate::json_fragment::Window<'_> {
        crate::json_fragment::Window {
            offset_bytes: self.offset_bytes,
            limit_bytes: self.limit_bytes,
            representation_digest: self.representation_digest.as_deref(),
        }
    }
}

pub(crate) fn properties() -> Value {
    json!({"offset_bytes":{"type":"integer","minimum":0},"limit_bytes":{"type":"integer","minimum":1,"maximum":4096},"representation_digest":{"type":"string","pattern":"^[a-f0-9]{64}$"}})
}

pub(crate) fn add_schema(schema: &mut Value) {
    if let Some(properties) = schema.get_mut("properties").and_then(Value::as_object_mut) {
        properties.extend(self::properties().as_object().unwrap().clone());
    }
    if let Some(variants) = schema.get_mut("oneOf").and_then(Value::as_array_mut) {
        for variant in variants {
            add_schema(variant);
        }
    }
}

pub(crate) fn revision(expected: Option<i64>, actual: i64, path: &'static str) -> Result<()> {
    if expected.is_some_and(|expected| expected != actual) {
        return Err(Error::refused_at(
            RefusalCode::StaleRevision,
            "PLANNING-READ-REVISION-01",
            path,
            "current authorized object revision",
            actual.to_string(),
            "restart_read_at_offset_zero",
            "current_object_revision",
        ));
    }
    Ok(())
}

pub(crate) fn help(request: crate::api::HelpRequest, capacity: usize) -> Result<Value> {
    let (request, arguments, window) = match request {
        crate::api::HelpRequest::Window {
            request,
            arguments,
            window,
        } => (*request, arguments, window),
        request => {
            let arguments = crate::api::help_arguments(&request);
            (request, arguments, Window::default())
        }
    };
    let value = crate::api::help(request)?;
    crate::json_fragment::encode(
        &value,
        vec![],
        capacity,
        window.borrowed(),
        json!({"tool":"help","selectors":arguments}),
        "help",
        arguments,
    )
    .map_err(help_error)
}

pub(crate) fn schema(mut schema: Value) -> Value {
    add_schema(&mut schema);
    schema
}

pub(crate) fn extract(arguments: &mut Value) -> Result<Window> {
    let mut fields = serde_json::Map::new();
    for name in ["offset_bytes", "limit_bytes", "representation_digest"] {
        if let Some(value) = arguments
            .as_object_mut()
            .and_then(|arguments| arguments.remove(name))
        {
            if value.is_null() {
                return Err(Error::InvalidArguments);
            }
            fields.insert(name.into(), value);
        }
    }
    let window: Window =
        serde_json::from_value(Value::Object(fields)).map_err(Error::invalid_arguments_from)?;
    window.validate()?;
    Ok(window)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn help_methods_reassemble_losslessly_with_real_parser_continuations() {
        for method in [
            "tectd-program",
            "tectd-scope-candidates",
            "tectd-slice-candidates",
        ] {
            let initial = json!({"mode":"describe","method":method});
            let full = crate::api::help(crate::api::parse_help(initial.clone()).unwrap()).unwrap();
            let expected = serde_json::to_vec(&full).unwrap();
            let mut arguments = initial;
            let mut assembled = Vec::new();
            let mut maximum = 0;
            loop {
                let page = help(crate::api::parse_help(arguments.clone()).unwrap(), 8192).unwrap();
                let bytes = crate::responses::encoded_len(&page).unwrap();
                maximum = maximum.max(bytes);
                assert!(bytes <= 8192);
                assert_eq!(
                    page,
                    help(crate::api::parse_help(arguments).unwrap(), 8192).unwrap()
                );
                if page["kind"] != "fragment" {
                    assert_eq!(page["body"], full["body"]);
                    break;
                }
                assert!(page["returned_bytes"].as_u64().unwrap() <= 4096);
                assert_eq!(
                    page["offset_bytes"].as_u64().unwrap() as usize,
                    assembled.len(),
                    "fragment must advance at the requested byte offset"
                );
                assert!(
                    page["returned_bytes"].as_u64().unwrap() > 0
                        || assembled.len() == expected.len()
                );
                assembled.extend_from_slice(page["text"].as_str().unwrap().as_bytes());
                let Some(action) = page["actions"].as_array().unwrap().first() else {
                    assert_eq!(assembled, expected);
                    break;
                };
                assert_eq!(action["tool"], "help");
                assert!(action.get("route_contract").is_none());
                arguments = action["arguments"].clone();
                crate::api::decode_public_call("help", arguments.clone()).unwrap();
                assert_eq!(arguments["method"], method);
            }
            println!("help[{method}] max_envelope_bytes={maximum}");
        }
    }

    #[test]
    fn planning_windows_are_real_read_params_and_rejected_on_mutations() {
        let id = uuid::Uuid::new_v4();
        for route in ["program.get", "scope.candidates.context"] {
            let mut params = if route == "program.get" {
                json!({"program_id":id})
            } else {
                json!({"candidate_set_id":id,"view":"overview","limit":25})
            };
            params["limit_bytes"] = json!(256);
            crate::api::decode_public_call("query", json!({"route":route,"params":params}))
                .unwrap();
        }
        for params in [
            json!({"mode":"describe","method":"tectd-program","offset_bytes":1}),
            json!({"mode":"search","limit_bytes":0}),
            json!({"mode":"search","representation_digest":null}),
        ] {
            assert!(crate::api::parse_help(params).is_err());
        }
        assert!(crate::api::decode_public_call("command", json!({"route":"program.save","params":{"program_id":id,"revision":1,"input_cursor":0,"limit_bytes":256}})).is_err());
        assert!(
            revision(Some(2), 3, "arguments.params.program_revision")
                .unwrap_err()
                .refusal()
                .is_some()
        );
    }
}

#[cfg(test)]
mod cache_tests {
    use super::*;
    #[test]
    fn compiled_method_cache_isolated_by_exact_selector_and_stable_when_reordered() {
        let methods = [
            "tectd-program",
            "tectd-setup",
            "tectd-scope-candidates",
            "tectd-slice-candidates",
        ];
        let mut values = Vec::new();
        for method in methods {
            let value = crate::api::help(
                crate::api::parse_help(json!({"mode":"describe","method":method})).unwrap(),
            )
            .unwrap();
            assert_eq!(value["method"], method);
            let expected = match method {
                "tectd-program" => include_str!("../../../skills/tectd-program/SKILL.md"),
                "tectd-setup" => include_str!("../../../skills/tectd-setup/SKILL.md"),
                "tectd-scope-candidates" => {
                    include_str!("../../../skills/tectd-scope-candidates/SKILL.md")
                }
                "tectd-slice-candidates" => {
                    include_str!("../../../skills/tectd-slice-candidates/SKILL.md")
                }
                _ => unreachable!(),
            };
            assert_eq!(value["body"], expected);
            values.push(value);
        }
        for index in [3, 1, 2, 0] {
            let repeated = crate::api::help(
                crate::api::parse_help(json!({"mode":"describe","method":methods[index]})).unwrap(),
            )
            .unwrap();
            assert_eq!(repeated, values[index]);
        }
        assert!(crate::api::parse_help(json!({"mode":"describe","method":"unknown"})).is_err());
    }
}

pub(crate) fn help_error(error: Error) -> Error {
    if let Error::Refused(mut refusal) = error {
        if let Some(path) = &mut refusal.path {
            *path = path.replace("arguments.params.", "arguments.");
        }
        Error::Refused(refusal)
    } else {
        error
    }
}
