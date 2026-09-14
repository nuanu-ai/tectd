use super::catalog::RouteSpec;
use serde_json::{Map, Value, json};

fn uuid() -> Value {
    json!({"type":"string","format":"uuid"})
}

fn binding() -> Value {
    json!({"oneOf":[
        {"type":"object","properties":{"kind":{"const":"workspace"}},"required":["kind"],"additionalProperties":false},
        {"type":"object","properties":{"kind":{"const":"program"},"program_id":uuid()},"required":["kind","program_id"],"additionalProperties":false},
        {"type":"object","properties":{"kind":{"const":"scope"},"scope_id":uuid()},"required":["kind","scope_id"],"additionalProperties":false},
        {"type":"object","properties":{"kind":{"const":"slice"},"scope_id":uuid(),"slice_id":uuid()},"required":["kind","scope_id","slice_id"],"additionalProperties":false},
        {"type":"object","properties":{"kind":{"const":"slice_phase"},"scope_id":uuid(),"slice_id":uuid(),"phase_id":{"type":"string","minLength":1,"maxLength":256}},"required":["kind","scope_id","slice_id","phase_id"],"additionalProperties":false}
    ]})
}

fn common() -> Map<String, Value> {
    serde_json::from_value(json!({
        "mode":{"type":"string","enum":["lexical","graph_search","super_wide"]},
        "limit":{"type":"integer","minimum":1,"maximum":50,"default":20},
        "corpus_limit":{"type":"integer","minimum":1,"maximum":512,"default":256},
        "binding":binding(),
        "kinds":{"type":"array","items":{"type":"string","enum":["constraint","claim","decision","hypothesis","procedure","protocol","infrastructure","operating_model","product_research","security"]},"maxItems":10,"uniqueItems":true},
        "purpose":{"type":"string","minLength":1,"maxLength":1024}
    })).expect("object")
}

fn variant(mode: &str, fields: &[&str], required: &[&str], graph: Option<bool>) -> Value {
    let mut properties = common();
    properties.insert("mode".into(), json!({"const":mode}));
    for field in fields {
        let schema = match *field {
            "query" => json!({"type":"string","minLength":1,"maxLength":2048}),
            "seeds" => {
                json!({"type":"array","items":{"type":"string","minLength":1,"maxLength":4096,"format":"uri"},"maxItems":16,"uniqueItems":true})
            }
            "relations" => {
                json!({"type":"array","items":{"type":"string","enum":["targets","depends_on","uses_asset","in_environment","derived_from","bound_to"]},"minItems":1,"maxItems":6,"uniqueItems":true})
            }
            "direction" => json!({"type":"string","enum":["outgoing","incoming","both"]}),
            "max_depth" => json!({"type":"integer","minimum":1,"maximum":4,"default":2}),
            _ => unreachable!(),
        };
        properties.insert((*field).into(), schema);
    }
    if mode == "graph_search" {
        properties["seeds"]["minItems"] = json!(1);
    }
    if let Some(include) = graph {
        properties.insert("include_graph".into(), json!({"const":include}));
    }
    json!({"type":"object","properties":properties,"required":required,"additionalProperties":false})
}

pub(super) fn route() -> RouteSpec {
    RouteSpec {
        tool: "query",
        route: "knowledge.search",
        internal: "knowledge_search",
        summary: "Search current visible canonical knowledge with bounded lexical, graph, or super-wide retrieval.",
        conditions: "Requires an authenticated open native session. Lexical and super_wide require query. graph_search requires seeds, relations, and direction. Graph parameters are accepted only when graph traversal is active; omitted limits use the advertised bounds.",
        effects: "Reads current authorized canonical knowledge only. Vector enrichment is local and optional; an unavailable or invalid local model returns lexical and requested graph candidates with vector_status vector_unavailable.",
        retry: "Safe to repeat. Results report corpus, result, graph-depth, and vector bounds explicitly.",
        schema: json!({"oneOf":[
            variant("lexical", &["query"], &["mode","query","purpose"], Some(false)),
            variant("graph_search", &["seeds","relations","direction","max_depth"], &["mode","seeds","relations","direction","purpose"], Some(false)),
            variant("super_wide", &["query"], &["mode","query","purpose"], Some(false)),
            variant("super_wide", &["query","seeds","relations","direction","max_depth"], &["mode","query","relations","direction","include_graph","purpose"], Some(true))
        ]}),
        example: json!({"mode":"super_wide","query":"current source provenance constraints","purpose":"Find current constraints relevant to the active change.","limit":20,"corpus_limit":256}),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_exposes_all_modes_bounds_and_strict_variants() {
        let route = route();
        let variants = route.schema["oneOf"].as_array().unwrap();
        assert_eq!(variants.len(), 4);
        assert!(
            variants
                .iter()
                .all(|value| value["additionalProperties"] == false)
        );
        assert_eq!(variants[0]["properties"]["limit"]["maximum"], 50);
        assert_eq!(variants[1]["properties"]["max_depth"]["maximum"], 4);
        assert_eq!(variants[1]["properties"]["seeds"]["minItems"], 1);
        assert_eq!(variants[3]["properties"]["include_graph"]["const"], true);
        assert!(variants[0]["properties"].get("direction").is_none());
        assert!(variants[1]["properties"].get("query").is_none());
        for invalid in [
            json!({"mode":"lexical","query":"x","direction":"both","purpose":"p"}),
            json!({"mode":"graph_search","query":"x","seeds":["urn:a"],"relations":["targets"],"direction":"both","purpose":"p"}),
            json!({"mode":"graph_search","seeds":[],"relations":["targets"],"direction":"both","purpose":"p"}),
        ] {
            assert!(
                crate::api::decode_public_call(
                    "query",
                    json!({"route":"knowledge.search","params":invalid})
                )
                .is_err()
            );
        }
        let described = crate::api::help(
            crate::api::parse_help(
                json!({"mode":"describe","tool":"query","route":"knowledge.search"}),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(described["params_schema"], route.schema);
        assert_eq!(
            described["example"]["arguments"]["route"],
            "knowledge.search"
        );
        let alias = crate::api::help(
            crate::api::parse_help(json!({"mode":"search","text":"найти knowledge"})).unwrap(),
        )
        .unwrap();
        assert_eq!(alias["hits"][0]["route"], "knowledge.search");
    }
}
