use super::*;

pub(crate) fn parse_help(mut arguments: Value) -> Result<HelpRequest> {
    if arguments.get("query").is_some() {
        return Err(Error::refused_at(
            tect_domain::RefusalCode::InputSchemaInvalid,
            "WP6-HELP-TEXT-SELECTOR",
            "arguments.query",
            "text field for a help selector",
            "unknown query field",
            "use_help_text",
            "text",
        ));
    }
    let original = arguments.clone();
    if arguments.get("mode").is_none() && arguments["text"] == "response-rules" {
        arguments["mode"] = json!("search");
    }
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
    if !fields.is_empty() {
        let window: crate::planning_read::Window =
            serde_json::from_value(Value::Object(fields)).map_err(Error::invalid_arguments_from)?;
        window
            .validate()
            .map_err(crate::planning_read::help_error)?;
        return Ok(HelpRequest::Window {
            request: Box::new(parse_help(arguments)?),
            arguments: original,
            window,
        });
    }
    for field in ["text", "tool", "route", "method"] {
        if arguments.get(field).is_some_and(Value::is_null) {
            return Err(Error::InvalidArguments);
        }
    }
    let args: HelpArguments =
        serde_json::from_value(arguments).map_err(Error::invalid_arguments_from)?;
    if args.text.as_ref().is_some_and(|text| text.contains('\0'))
        || args
            .tool
            .as_ref()
            .is_some_and(|tool| !PUBLIC_TOOLS.contains(&tool.as_str()))
        || args.method.as_ref().is_some_and(|method| {
            !matches!(
                method.as_str(),
                "tectd-program"
                    | "tectd-setup"
                    | "tectd-scope-candidates"
                    | "tectd-slice-candidates"
            )
        })
    {
        return Err(Error::InvalidArguments);
    }
    match args.mode.as_str() {
        "search" if args.route.is_none() && args.method.is_none() => Ok(HelpRequest::Search {
            text: args.text,
            tool: args.tool,
        }),
        "describe" if args.text.is_none() => match (args.tool, args.route, args.method) {
            (Some(tool), None, None) => Ok(HelpRequest::DescribeTool(tool)),
            (Some(tool), Some(route), None)
                if matches!(tool.as_str(), "query" | "command" | "execute") =>
            {
                Ok(HelpRequest::DescribeRoute(
                    route_for(&tool, &route).ok_or(Error::InvalidArguments)?,
                ))
            }
            (None, None, Some(method)) => Ok(HelpRequest::DescribeMethod(method)),
            _ => Err(Error::InvalidArguments),
        },
        _ => Err(Error::InvalidArguments),
    }
}

pub(crate) fn help(request: HelpRequest) -> Result<Value> {
    Ok(match request {
        HelpRequest::Window { request, .. } => help(*request)?,
        HelpRequest::Search { text, tool }
            if text.as_deref() == Some("response-rules") && tool.is_none() =>
        {
            json!({"kind":"response_rules","text":"response-rules","body":crate::responses::RESPONSE_RULES})
        }
        HelpRequest::Search { text, tool } => search(text.as_deref(), tool.as_deref()),
        HelpRequest::DescribeTool(tool) => describe_tool(&tool),
        HelpRequest::DescribeRoute(spec) => describe_route(&spec),
        HelpRequest::DescribeMethod(method) => {
            // These method contracts are compiled immutable data. Cache their
            // complete representation so each byte-window read avoids rebuilding
            // and revalidating the same static pipeline catalogue.
            static PROGRAM: std::sync::OnceLock<Value> = std::sync::OnceLock::new();
            static SETUP: std::sync::OnceLock<Value> = std::sync::OnceLock::new();
            static SCOPE: std::sync::OnceLock<Value> = std::sync::OnceLock::new();
            static SLICE: std::sync::OnceLock<Value> = std::sync::OnceLock::new();
            let cache = match method.as_str() {
                "tectd-program" => &PROGRAM,
                "tectd-setup" => &SETUP,
                "tectd-scope-candidates" => &SCOPE,
                "tectd-slice-candidates" => &SLICE,
                _ => return Err(Error::InternalInvariant),
            };
            if let Some(value) = cache.get() {
                return Ok(value.clone());
            }
            let (description, body) = match method.as_str() {
                "tectd-program" => (
                    "Method for forming and continuing a durable Program PRD.",
                    PROGRAM_METHOD,
                ),
                "tectd-setup" => (
                    "Method for composing and safely applying initial workspace instructions.",
                    SETUP_METHOD,
                ),
                "tectd-scope-candidates" => (
                    "Method for deriving, reviewing, and continuing durable Scope candidates.",
                    SCOPE_CANDIDATE_METHOD,
                ),
                "tectd-slice-candidates" => (
                    "Method for designing and reviewing a complete revisable Slice-candidate plan.",
                    SLICE_CANDIDATE_METHOD,
                ),
                _ => return Err(Error::InternalInvariant),
            };
            let mut value = json!({"mode":"describe","kind":"method","method":method,
                "tool":"help","description":description,"body":body});
            if method == "tectd-program" {
                value["method_revision"] = json!(crate::program_output::PROGRAM_METHOD_REVISION);
            }
            if matches!(method.as_str(), "tectd-program" | "tectd-scope-candidates") {
                use sha2::Digest;
                value["method_digest"] =
                    json!(format!("{:x}", sha2::Sha256::digest(body.as_bytes())));
                let revision = if method == "tectd-program" {
                    crate::program_output::PROGRAM_METHOD_REVISION
                } else {
                    crate::scope_guidance::METHOD_REVISION
                };
                value["origin_refs"] = json!([format!("skills/{method}/SKILL.md@{revision}")]);
            }
            if method == "tectd-scope-candidates" {
                value["method_revision"] = json!(crate::scope_guidance::METHOD_REVISION);
                value["guidance_registry"] = crate::scope_guidance::help_registry()?;
            }
            if method == "tectd-slice-candidates" {
                value["method_revision"] = json!(crate::slice_guidance::METHOD_REVISION);
                let details = crate::slice_guidance::help()?;
                value["guidance_registry"] = details["guidance_registry"].clone();
                value["pipeline_catalog"] = details["pipeline_catalog"].clone();
            }
            let _ = cache.set(value.clone());
            value
        }
    })
}

pub(crate) fn help_arguments(request: &HelpRequest) -> Value {
    match request {
        HelpRequest::Search { text, tool } => {
            let mut args = json!({"mode":"search"});
            if let Some(text) = text {
                args["text"] = json!(text);
            }
            if let Some(tool) = tool {
                args["tool"] = json!(tool);
            }
            args
        }
        HelpRequest::DescribeTool(tool) => json!({"mode":"describe","tool":tool}),
        HelpRequest::DescribeRoute(spec) => {
            json!({"mode":"describe","tool":spec.tool,"route":spec.route})
        }
        HelpRequest::DescribeMethod(method) => json!({"mode":"describe","method":method}),
        HelpRequest::Window { arguments, .. } => arguments.clone(),
    }
}
