use oxrdf::{Literal, NamedNode, NamedOrBlankNode, Term, Triple};
use std::collections::BTreeSet;
use tect_domain::{Error, Result};

pub(super) const RDF_TYPE: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";
pub(super) const XSD_STRING: &str = "http://www.w3.org/2001/XMLSchema#string";
pub(super) const XSD_INTEGER: &str = "http://www.w3.org/2001/XMLSchema#integer";
pub(super) const XSD_BOOLEAN: &str = "http://www.w3.org/2001/XMLSchema#boolean";
pub(super) const XSD_DATETIME: &str = "http://www.w3.org/2001/XMLSchema#dateTime";
pub(super) const DK: &str = "urn:tect:dk:";
pub(super) const V2: &str = "urn:tect:dk:v2:";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RdfRefs {
    pub unit: String,
    pub revision: String,
    pub event: String,
    pub event_content: String,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct TypedTerm {
    pub kind: String,
    pub value: String,
    pub datatype: Option<String>,
    pub language: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct TypedTriple {
    pub subject: TypedTerm,
    pub predicate: TypedTerm,
    pub object: TypedTerm,
}

#[derive(Debug, Clone)]
pub(crate) struct RdfDocument {
    pub payload: String,
    pub stable_payload: String,
    pub refs: RdfRefs,
    pub(super) triples: BTreeSet<TypedTriple>,
}

pub(super) struct Builder {
    pub refs: RdfRefs,
    triples: BTreeSet<TypedTriple>,
}

impl Builder {
    pub fn new(refs: RdfRefs) -> Self {
        Self {
            refs,
            triples: BTreeSet::new(),
        }
    }

    pub fn iri(&mut self, subject: &str, predicate: &str, object: &str) -> Result<()> {
        NamedNode::new(subject).map_err(|_| Error::InvalidArguments)?;
        NamedNode::new(predicate).map_err(|_| Error::InvalidArguments)?;
        NamedNode::new(object).map_err(|_| Error::InvalidArguments)?;
        self.insert(subject, predicate, TypedTerm::iri(object));
        Ok(())
    }

    pub fn text(&mut self, subject: &str, predicate: &str, value: &str) -> Result<()> {
        self.typed(subject, predicate, value, XSD_STRING)
    }

    pub fn integer(&mut self, subject: &str, predicate: &str, value: i64) -> Result<()> {
        self.typed(subject, predicate, &value.to_string(), XSD_INTEGER)
    }

    pub fn boolean(&mut self, subject: &str, predicate: &str, value: bool) -> Result<()> {
        self.typed(
            subject,
            predicate,
            if value { "true" } else { "false" },
            XSD_BOOLEAN,
        )
    }

    pub fn datetime(&mut self, subject: &str, predicate: &str, value: &str) -> Result<()> {
        self.typed(subject, predicate, value, XSD_DATETIME)
    }

    fn typed(&mut self, subject: &str, predicate: &str, value: &str, datatype: &str) -> Result<()> {
        NamedNode::new(subject).map_err(|_| Error::InvalidArguments)?;
        NamedNode::new(predicate).map_err(|_| Error::InvalidArguments)?;
        NamedNode::new(datatype).map_err(|_| Error::InvalidArguments)?;
        self.insert(subject, predicate, TypedTerm::literal(value, datatype));
        Ok(())
    }

    fn insert(&mut self, subject: &str, predicate: &str, object: TypedTerm) {
        self.triples.insert(TypedTriple {
            subject: TypedTerm::iri(subject),
            predicate: TypedTerm::iri(predicate),
            object,
        });
    }

    pub fn finish(self, omit_legacy_unit_type_from_stable: bool) -> Result<RdfDocument> {
        let payload = render(&self.triples)?;
        let stable = if omit_legacy_unit_type_from_stable {
            self.triples
                .iter()
                .filter(|triple| {
                    !(triple.subject.value == self.refs.unit
                        && triple.predicate.value == RDF_TYPE
                        && triple.object.value == format!("{DK}KnowledgeUnit"))
                })
                .cloned()
                .collect()
        } else {
            self.triples.clone()
        };
        Ok(RdfDocument {
            payload,
            stable_payload: render(&stable)?,
            refs: self.refs,
            triples: self.triples,
        })
    }
}

impl TypedTerm {
    fn iri(value: &str) -> Self {
        Self {
            kind: "iri".into(),
            value: value.into(),
            datatype: None,
            language: None,
        }
    }

    fn literal(value: &str, datatype: &str) -> Self {
        Self {
            kind: "literal".into(),
            value: value.into(),
            datatype: Some(datatype.into()),
            language: None,
        }
    }
}

fn render(values: &BTreeSet<TypedTriple>) -> Result<String> {
    let mut output = String::new();
    for value in values {
        let object: Term = if value.object.kind == "iri" {
            NamedNode::new(&value.object.value)
                .map_err(|_| Error::InvalidArguments)?
                .into()
        } else {
            Literal::new_typed_literal(
                &value.object.value,
                NamedNode::new(value.object.datatype.as_deref().unwrap_or(XSD_STRING))
                    .map_err(|_| Error::InvalidArguments)?,
            )
            .into()
        };
        let triple = Triple::new(
            NamedOrBlankNode::NamedNode(
                NamedNode::new(&value.subject.value).map_err(|_| Error::InvalidArguments)?,
            ),
            NamedNode::new(&value.predicate.value).map_err(|_| Error::InvalidArguments)?,
            object,
        );
        output.push_str(&format!("{triple} .\n"));
    }
    Ok(output)
}

fn row_term(row: &serde_json::Value, key: &str) -> Result<TypedTerm> {
    let value = row.get(key).ok_or(Error::InternalInvariant)?;
    let kind = value
        .get("type")
        .and_then(|v| v.as_str())
        .ok_or(Error::InternalInvariant)?;
    if !matches!(kind, "iri" | "literal") {
        return Err(Error::InternalInvariant);
    }
    Ok(TypedTerm {
        kind: kind.into(),
        value: value
            .get("value")
            .and_then(|v| v.as_str())
            .ok_or(Error::InternalInvariant)?
            .into(),
        datatype: value
            .get("datatype")
            .and_then(|v| v.as_str())
            .map(Into::into)
            .or_else(|| (kind == "literal").then(|| XSD_STRING.into())),
        language: value
            .get("language")
            .and_then(|v| v.as_str())
            .map(Into::into),
    })
}

pub(super) fn validate_rows(rows: &[serde_json::Value], document: &RdfDocument) -> Result<()> {
    let actual = rows
        .iter()
        .map(|row| {
            Ok(TypedTriple {
                subject: row_term(row, "subject")?,
                predicate: row_term(row, "predicate")?,
                object: row_term(row, "object")?,
            })
        })
        .collect::<Result<BTreeSet<_>>>()?;
    if actual.len() == rows.len() && actual == document.triples {
        Ok(())
    } else {
        Err(Error::InternalInvariant)
    }
}
