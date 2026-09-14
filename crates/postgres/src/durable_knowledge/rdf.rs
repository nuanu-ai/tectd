use super::*;
use oxrdf::{Literal, NamedNode, NamedOrBlankNode, Term, Triple};
use std::collections::BTreeSet;

const RDF_TYPE: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";
const XSD_STRING: &str = "http://www.w3.org/2001/XMLSchema#string";
const DK: &str = "urn:tect:dk:";

#[derive(Debug, Clone)]
pub(crate) struct RdfRefs {
    pub unit: String,
    pub revision: String,
    pub source: String,
    pub event: String,
}
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct TypedTerm {
    kind: String,
    value: String,
    datatype: Option<String>,
    language: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct TypedTriple {
    subject: TypedTerm,
    predicate: TypedTerm,
    object: TypedTerm,
}
pub(crate) struct RdfDocument {
    pub payload: String,
    pub stable_payload: String,
    pub refs: RdfRefs,
    triples: BTreeSet<TypedTriple>,
}

pub(crate) fn refs(
    tenant: Uuid,
    workspace: Uuid,
    unit: Uuid,
    revision: i64,
    event: Uuid,
) -> RdfRefs {
    let unit = format!("urn:tect:dk:unit:{tenant}:{workspace}:{unit}");
    let revision_ref = format!("{unit}:revision:{revision}");
    RdfRefs {
        source: format!("{revision_ref}:source"),
        event: format!("urn:tect:dk:event:{tenant}:{workspace}:{event}"),
        unit,
        revision: revision_ref,
    }
}
fn node(value: &str) -> Result<NamedNode> {
    NamedNode::new(value).map_err(|_| Error::InvalidArguments)
}
fn iri_term(value: &str) -> TypedTerm {
    TypedTerm {
        kind: "iri".into(),
        value: value.into(),
        datatype: None,
        language: None,
    }
}
fn literal_term(value: &str) -> TypedTerm {
    TypedTerm {
        kind: "literal".into(),
        value: value.into(),
        datatype: Some(XSD_STRING.into()),
        language: None,
    }
}
fn add(
    doc: &mut RdfDocument,
    subject: &str,
    predicate: &str,
    object: &str,
    is_iri: bool,
) -> Result<()> {
    let object_term: Term = if is_iri {
        node(object)?.into()
    } else {
        Literal::new_simple_literal(object).into()
    };
    let rendered = Triple::new(
        NamedOrBlankNode::NamedNode(node(subject)?),
        node(predicate)?,
        object_term,
    );
    let typed = TypedTriple {
        subject: iri_term(subject),
        predicate: iri_term(predicate),
        object: if is_iri {
            iri_term(object)
        } else {
            literal_term(object)
        },
    };
    if doc.triples.insert(typed) {
        doc.payload.push_str(&format!("{rendered} .\n"));
    }
    Ok(())
}
fn add_event(
    doc: &mut RdfDocument,
    op: KnowledgeOperation,
    reason: &str,
    authority: &str,
    principal: Uuid,
    session: Uuid,
) -> Result<()> {
    let e = doc.refs.event.clone();
    let u = doc.refs.unit.clone();
    let r = doc.refs.revision.clone();
    add(doc, &e, RDF_TYPE, &format!("{DK}PublicationEvent"), true)?;
    add(doc, &e, &format!("{DK}unit"), &u, true)?;
    add(doc, &e, &format!("{DK}revisionRef"), &r, true)?;
    add(
        doc,
        &e,
        &format!("{DK}operation"),
        &format!("{DK}{}", operation(op)),
        true,
    )?;
    add(doc, &e, &format!("{DK}reason"), reason, false)?;
    add(doc, &e, &format!("{DK}authorityBasis"), authority, false)?;
    add(
        doc,
        &e,
        &format!("{DK}actorPrincipal"),
        &format!("urn:tect:principal:{principal}"),
        true,
    )?;
    add(
        doc,
        &e,
        &format!("{DK}actorSession"),
        &format!("urn:tect:session:{session}"),
        true,
    )?;
    add(
        doc,
        &e,
        &format!("{DK}profile"),
        &format!("{DK}general_constraint"),
        true,
    )?;
    add(
        doc,
        &e,
        &format!("{DK}profileVersion"),
        DK_PROFILE_VERSION,
        false,
    )
}

#[allow(clippy::too_many_arguments)]
fn build_revision(
    refs: RdfRefs,
    op: KnowledgeOperation,
    draft: &KnowledgeConstraintDraft,
    provenance: Option<&KnowledgeBindingProvenance>,
    source_sha: &str,
    reason: &str,
    authority: &str,
    principal: Uuid,
    session: Uuid,
) -> Result<RdfDocument> {
    let mut doc = RdfDocument {
        payload: String::new(),
        stable_payload: String::new(),
        refs,
        triples: BTreeSet::new(),
    };
    let u = doc.refs.unit.clone();
    let r = doc.refs.revision.clone();
    let s = doc.refs.source.clone();
    add(&mut doc, &u, RDF_TYPE, &format!("{DK}KnowledgeUnit"), true)?;
    add(&mut doc, &u, &format!("{DK}hasRevision"), &r, true)?;
    let revision = r.rsplit(':').next().ok_or(Error::InternalInvariant)?;
    add(
        &mut doc,
        &r,
        RDF_TYPE,
        &format!("{DK}ConstraintRevision"),
        true,
    )?;
    add(&mut doc, &r, &format!("{DK}unit"), &u, true)?;
    add(&mut doc, &r, &format!("{DK}revision"), revision, false)?;
    add(&mut doc, &r, &format!("{DK}title"), &draft.title, false)?;
    add(
        &mut doc,
        &r,
        &format!("{DK}statement"),
        &draft.statement,
        false,
    )?;
    add(
        &mut doc,
        &r,
        &format!("{DK}modality"),
        &format!(
            "{DK}{}",
            match draft.modality {
                KnowledgeModality::Must => "must",
                KnowledgeModality::MustNot => "must_not",
            }
        ),
        true,
    )?;
    add(&mut doc, &r, &format!("{DK}action"), &draft.action, false)?;
    add(
        &mut doc,
        &r,
        &format!("{DK}target"),
        &draft.target_iri,
        true,
    )?;
    add(
        &mut doc,
        &r,
        &format!("{DK}conditionCount"),
        &draft.conditions.len().to_string(),
        false,
    )?;
    add(
        &mut doc,
        &r,
        &format!("{DK}exceptionCount"),
        &draft.exceptions.len().to_string(),
        false,
    )?;
    for v in &draft.conditions {
        add(&mut doc, &r, &format!("{DK}condition"), v, false)?
    }
    for v in &draft.exceptions {
        add(&mut doc, &r, &format!("{DK}exception"), v, false)?
    }
    add(&mut doc, &r, &format!("{DK}source"), &s, true)?;
    add(
        &mut doc,
        &r,
        &format!("{DK}purpose"),
        &format!("{DK}execution_constraint"),
        true,
    )?;
    add(
        &mut doc,
        &r,
        &format!("{DK}versionResolution"),
        &format!("{DK}current_accepted"),
        true,
    )?;
    match &draft.binding {
        KnowledgeBinding::Workspace => add(
            &mut doc,
            &r,
            &format!("{DK}bindingKind"),
            &format!("{DK}workspace"),
            true,
        )?,
        KnowledgeBinding::SlicePhase {
            scope_id,
            slice_id,
            phase_id,
        } => {
            add(
                &mut doc,
                &r,
                &format!("{DK}bindingKind"),
                &format!("{DK}slice_phase"),
                true,
            )?;
            add(
                &mut doc,
                &r,
                &format!("{DK}scopeId"),
                &scope_id.to_string(),
                false,
            )?;
            add(
                &mut doc,
                &r,
                &format!("{DK}sliceId"),
                &slice_id.to_string(),
                false,
            )?;
            add(&mut doc, &r, &format!("{DK}phaseId"), phase_id, false)?;
        }
    }
    if let Some(v) = provenance {
        add(
            &mut doc,
            &r,
            &format!("{DK}definitionKind"),
            v.definition_kind.as_str(),
            false,
        )?;
        add(
            &mut doc,
            &r,
            &format!("{DK}definitionVersion"),
            &v.definition_version,
            false,
        )?;
        add(
            &mut doc,
            &r,
            &format!("{DK}definitionDigest"),
            &v.definition_digest,
            false,
        )?;
    }
    add(&mut doc, &s, RDF_TYPE, &format!("{DK}SourceFragment"), true)?;
    add(
        &mut doc,
        &s,
        &format!("{DK}title"),
        &draft.source.title,
        false,
    )?;
    add(&mut doc, &s, &format!("{DK}uri"), &draft.source.uri, false)?;
    add(
        &mut doc,
        &s,
        &format!("{DK}text"),
        &draft.source.text,
        false,
    )?;
    add(&mut doc, &s, &format!("{DK}sha256"), source_sha, false)?;
    add_event(&mut doc, op, reason, authority, principal, session)?;
    doc.stable_payload = if op == KnowledgeOperation::Revise {
        let prefix = format!("<{u}> <{RDF_TYPE}>");
        doc.payload
            .lines()
            .filter(|line| !line.starts_with(&prefix))
            .map(|line| format!("{line}\n"))
            .collect()
    } else {
        doc.payload.clone()
    };
    Ok(doc)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn revision_document(
    tenant: Uuid,
    workspace: Uuid,
    unit: Uuid,
    revision: i64,
    event: Uuid,
    op: KnowledgeOperation,
    draft: &KnowledgeConstraintDraft,
    provenance: Option<&KnowledgeBindingProvenance>,
    source_sha: &str,
    reason: &str,
    authority: &str,
    principal: Uuid,
    session: Uuid,
) -> Result<RdfDocument> {
    build_revision(
        refs(tenant, workspace, unit, revision, event),
        op,
        draft,
        provenance,
        source_sha,
        reason,
        authority,
        principal,
        session,
    )
}
#[allow(clippy::too_many_arguments)]
pub(crate) fn event_document(
    tenant: Uuid,
    workspace: Uuid,
    unit: Uuid,
    revision: i64,
    event: Uuid,
    op: KnowledgeOperation,
    reason: &str,
    authority: &str,
    principal: Uuid,
    session: Uuid,
) -> Result<RdfDocument> {
    let refs = refs(tenant, workspace, unit, revision, event);
    let mut doc = RdfDocument {
        payload: String::new(),
        stable_payload: String::new(),
        refs,
        triples: BTreeSet::new(),
    };
    add_event(&mut doc, op, reason, authority, principal, session)?;
    doc.stable_payload = doc.payload.clone();
    Ok(doc)
}
pub(crate) async fn native_publish(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    event: Uuid,
    op: KnowledgeOperation,
    payload: &str,
    stable_payload: &str,
) -> Result<String> {
    sqlx::query_scalar("SELECT tect_dk_native_publish($1,$2,$3,$4,$5,$6)")
        .bind(tenant)
        .bind(workspace)
        .bind(event)
        .bind(operation(op))
        .bind(payload)
        .bind(stable_payload)
        .fetch_one(&mut **tx)
        .await
        .map_err(native_error)
}
pub(crate) async fn native_rows(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    unit: Uuid,
    revision: i64,
    event: Uuid,
) -> Result<Vec<serde_json::Value>> {
    sqlx::query_scalar("SELECT * FROM tect_dk_native_read($1,$2,$3,$4,$5)")
        .bind(tenant)
        .bind(workspace)
        .bind(unit)
        .bind(revision)
        .bind(event)
        .fetch_all(&mut **tx)
        .await
        .map_err(native_error)
}
fn row_term(row: &serde_json::Value, key: &str) -> Result<TypedTerm> {
    let v = row.get(key).ok_or(Error::InternalInvariant)?;
    let kind = v
        .get("type")
        .and_then(|v| v.as_str())
        .ok_or(Error::InternalInvariant)?;
    if !matches!(kind, "iri" | "literal") {
        return Err(Error::InternalInvariant);
    };
    Ok(TypedTerm {
        kind: kind.into(),
        value: v
            .get("value")
            .and_then(|v| v.as_str())
            .ok_or(Error::InternalInvariant)?
            .into(),
        datatype: v
            .get("datatype")
            .and_then(|v| v.as_str())
            .map(Into::into)
            .or_else(|| (kind == "literal").then(|| XSD_STRING.into())),
        language: v.get("language").and_then(|v| v.as_str()).map(Into::into),
    })
}
pub(crate) fn validate_rows(
    rows: &[serde_json::Value],
    value: &KnowledgeUnitRevision,
) -> Result<()> {
    let refs = RdfRefs {
        unit: value.unit_iri.clone(),
        revision: value.revision_iri.clone(),
        source: value.source_iri.clone(),
        event: value.publication_event_iri.clone(),
    };
    let expected = build_revision(
        refs,
        value.publication_operation,
        &value.constraint,
        value.binding_provenance.as_ref(),
        &value.source_sha256,
        &value.publication_reason,
        &value.publication_authority_basis,
        value.publication_actor_principal_id,
        value.publication_actor_session_id,
    )?
    .triples;
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
    if actual == expected {
        Ok(())
    } else {
        Err(Error::InternalInvariant)
    }
}

#[cfg(test)]
mod rdf_tests;
