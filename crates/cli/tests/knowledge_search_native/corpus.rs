use serde::Deserialize;
use serde_json::{Value, json};
use std::{collections::BTreeMap, fs, path::Path};
use uuid::Uuid;

#[derive(Debug, Deserialize)]
pub struct Corpus {
    pub records: Vec<Record>,
    pub queries: Vec<Query>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Record {
    pub id: String,
    pub title: String,
    pub canonical_text: String,
    pub epistemic_state: String,
    pub access_scope: String,
    pub binding_fixture: String,
    pub source: Source,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Source {
    pub path: String,
    pub sha256: String,
    pub revision: String,
    pub line: u32,
    pub text: String,
}

#[derive(Debug, Deserialize)]
pub struct Query {
    pub id: String,
    pub mode: String,
    pub query: String,
    pub relevant: Vec<String>,
    pub category: String,
}

pub fn load(path: &Path) -> Corpus {
    let bytes = fs::read(path).expect("root-authored DK3 corpus must be readable");
    serde_json::from_slice(&bytes).expect("root-authored DK3 corpus must remain valid JSON")
}

pub fn document(record: &Record, program_id: Uuid) -> Value {
    assert_eq!(record.epistemic_state, "declared");
    assert_eq!(record.access_scope, "workspace_members");
    assert_eq!(record.binding_fixture, "program_a_reference");
    assert_eq!(record.source.text, record.canonical_text);
    let resource = format!("urn:tect:dk3:corpus:resource:{}", record.id);
    json!({
        "title":record.title,
        "canonical_text":record.canonical_text,
        "knowledge_kind":"claim",
        "epistemic_state":"declared",
        "target_iris":[resource],
        "conditions":[],
        "exceptions":[],
        "sources":[{"kind":"snapshot","snapshot":{
            "title":record.title,
            "uri":source_iri(&record.id),
            "text":record.source.text,
            "evidence_kind":"declaration"
        }}],
        "bindings":[{
            "target":{"kind":"program","program_id":program_id},
            "purpose":"reference",
            "version_resolution":{"kind":"current_accepted"}
        }],
        "profiles":["general"],
        "access_scope":record.access_scope,
        "owner_ref":"isolated-dk3-acceptance-owner",
        "authority_basis":"Root-authored immutable DK3 retrieval corpus in an isolated acceptance fixture.",
        "sections":{
            "constraint":null,
            "general":{
                "statement":record.source.text,
                "assumptions":[],
                "evidence_scope":format!(
                    "Declared excerpt {}:{} at revision {} and sha256 {}.",
                    record.source.path,record.source.line,record.source.revision,record.source.sha256
                ),
                "rationale":"The exact source excerpt is the declared search fixture and remains linked to its source identity.",
                "alternatives":[],
                "negative_limits":["This fixture does not claim installed runtime or production behavior."],
                "unknown_limits":[]
            },
            "runbook":null,
            "protocol":null,
            "devops":null,
            "operations":null,
            "product_research":null,
            "security":null
        }
    })
}

pub fn source_iri(id: &str) -> String {
    format!("urn:tect:dk3:corpus:source:{id}")
}

pub fn search_params(query: &Query, mode: &str, binding: &Value) -> Value {
    json!({
        "mode":mode,
        "query":query.query,
        "binding":binding,
        "limit":16,
        "corpus_limit":32,
        "purpose":format!("Evaluate immutable DK3 corpus query {} ({})",query.id,query.category)
    })
}

pub fn result_units(response: &Value) -> Vec<Uuid> {
    response["results"]
        .as_array()
        .expect("search results")
        .iter()
        .map(|result| Uuid::parse_str(result["unit_id"].as_str().expect("unit ID")).expect("UUID"))
        .collect()
}

pub fn assert_current_results(response: &Value) {
    let results = response["results"].as_array().expect("search results");
    assert_eq!(response["bounds"]["results_returned"], results.len());
    for result in results {
        assert_eq!(result["revision"], 1);
        assert_eq!(result["lifecycle"], "active");
        assert_eq!(result["kind"], "claim");
        assert!(
            result["resource_iri"]
                .as_str()
                .is_some_and(|value| value.starts_with("urn:tect:dk:unit:"))
        );
        assert!(
            result["revision_iri"]
                .as_str()
                .is_some_and(|value| value.ends_with(":revision:1"))
        );
        assert!(!result["reasons"].as_array().unwrap().is_empty());
    }
}

pub fn ranking(response: &Value, reverse: &BTreeMap<Uuid, &str>, relevant: &[String]) -> Value {
    let order = result_units(response)
        .iter()
        .map(|unit| reverse.get(unit).copied().unwrap())
        .collect::<Vec<_>>();
    let hits = [1usize, 3, 5]
        .into_iter()
        .map(|cutoff| {
            let end = cutoff.min(order.len());
            (
                cutoff.to_string(),
                order[..end]
                    .iter()
                    .filter(|id| relevant.iter().any(|value| value == **id))
                    .count(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    json!({"order":order,"relevant_hits":hits})
}

pub fn incremental_top_five(
    query: &Query,
    lexical: &Value,
    wide: &Value,
    reverse: &BTreeMap<Uuid, &str>,
) -> Vec<Value> {
    let lexical_units = result_units(lexical);
    wide["results"]
        .as_array()
        .unwrap()
        .iter()
        .take(5)
        .enumerate()
        .filter_map(|(index, result)| {
            let unit = Uuid::parse_str(result["unit_id"].as_str().unwrap()).unwrap();
            let id = reverse.get(&unit).copied().unwrap();
            let vector = result["reasons"]
                .as_array()
                .unwrap()
                .iter()
                .any(|reason| reason["kind"] == "vector_similarity");
            (query.relevant.iter().any(|value| value == id)
                && !lexical_units.contains(&unit)
                && vector)
                .then(|| {
                    json!({"query_id":query.id,"target_id":id,"rank":index+1,
                    "reasons":result["reasons"]})
                })
        })
        .collect()
}
