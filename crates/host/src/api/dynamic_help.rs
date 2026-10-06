use super::catalog::routes;

pub(super) fn tool_summary(tool: &str) -> String {
    match tool {
        "get_state" => "Read bounded DB-only state for the current native session.".into(),
        "query" => format!(
            "Run one of {} named read-only routes.",
            routes().iter().filter(|spec| spec.tool == tool).count()
        ),
        "command" => format!(
            "Run one of {} named logical state-transition routes.",
            routes().iter().filter(|spec| spec.tool == tool).count()
        ),
        "execute" => format!(
            "Run one of {} explicit external-effect routes.",
            routes().iter().filter(|spec| spec.tool == tool).count()
        ),
        "help" => "Search or describe this API and its four embedded methods.".into(),
        _ => String::new(),
    }
}
