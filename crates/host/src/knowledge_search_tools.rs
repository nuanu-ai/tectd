use serde_json::Value;
use tect_domain::{Error, KnowledgeSearchQuery, Result};

pub(crate) fn parse(name: &str, arguments: Value) -> Result<KnowledgeSearchQuery> {
    if name != "knowledge_search" || contains_null(&arguments) || !mode_fields_valid(&arguments) {
        return Err(Error::InvalidArguments);
    }
    let query: KnowledgeSearchQuery =
        serde_json::from_value(arguments).map_err(|_| Error::InvalidArguments)?;
    query.validate()?;
    Ok(query)
}

fn mode_fields_valid(value: &Value) -> bool {
    let Some(fields) = value.as_object() else {
        return false;
    };
    let common = [
        "mode",
        "limit",
        "corpus_limit",
        "binding",
        "kinds",
        "purpose",
    ];
    let mode = fields.get("mode").and_then(Value::as_str);
    let graph = fields.get("include_graph") == Some(&Value::Bool(true));
    let specific: &[&str] = match (mode, graph) {
        (Some("lexical"), false) => &["query", "include_graph"],
        (Some("graph_search"), false) => &[
            "seeds",
            "relations",
            "direction",
            "max_depth",
            "include_graph",
        ],
        (Some("super_wide"), false) => &["query", "include_graph"],
        (Some("super_wide"), true) => &[
            "query",
            "seeds",
            "relations",
            "direction",
            "max_depth",
            "include_graph",
        ],
        _ => return false,
    };
    fields
        .keys()
        .all(|field| common.contains(&field.as_str()) || specific.contains(&field.as_str()))
}

fn contains_null(value: &Value) -> bool {
    match value {
        Value::Null => true,
        Value::Array(values) => values.iter().any(contains_null),
        Value::Object(values) => values.values().any(contains_null),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn exact_modes_and_omission_only_options_are_enforced() {
        assert!(
            parse(
                "knowledge_search",
                json!({"mode":"lexical","query":"точный запрос","purpose":"current evidence"})
            )
            .is_ok()
        );
        assert!(parse("knowledge_search", json!({"mode":"graph_search","seeds":["urn:unit:a"],"relations":["targets"],"direction":"both","purpose":"trace"})).is_ok());
        assert!(parse("knowledge_search", json!({"mode":"super_wide","query":"wide","include_graph":true,"relations":["derived_from"],"direction":"outgoing","purpose":"current evidence"})).is_ok());
        for invalid in [
            json!({"mode":"lexical","query":"x","direction":"both","purpose":"p"}),
            json!({"mode":"lexical","query":"x","seeds":[],"purpose":"p"}),
            json!({"mode":"graph_search","seeds":["relative"],"relations":["targets"],"direction":"both","purpose":"p"}),
            json!({"mode":"super_wide","query":"x","include_graph":true,"relations":[],"direction":"both","purpose":"p"}),
            json!({"mode":"lexical","query":null,"purpose":"p"}),
            json!({"mode":"lexical","query":"x","purpose":"p","sql":"select 1"}),
        ] {
            assert_eq!(
                parse("knowledge_search", invalid),
                Err(Error::InvalidArguments)
            );
        }
    }
}
