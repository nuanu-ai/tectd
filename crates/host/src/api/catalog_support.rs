use serde_json::{Value, json};

#[derive(Clone)]
pub(crate) struct RouteSpec {
    pub tool: &'static str,
    pub route: &'static str,
    pub internal: &'static str,
    pub summary: &'static str,
    pub conditions: &'static str,
    pub effects: &'static str,
    pub retry: &'static str,
    pub schema: Value,
    pub example: Value,
}

pub(super) fn uuid() -> Value {
    json!({"type":"string","format":"uuid"})
}

pub(super) fn text() -> Value {
    json!({"type":"string","minLength":1})
}

pub(super) fn nullable_text() -> Value {
    json!({"type":["string","null"]})
}

pub(super) fn page_limit() -> Value {
    json!({"type":"integer","minimum":1,"maximum":100,"default":25})
}
