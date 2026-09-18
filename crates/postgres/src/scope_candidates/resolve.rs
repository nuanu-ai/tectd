use crate::storage_error;
use sqlx::{Postgres, Transaction};
use std::collections::BTreeMap;
use tect_domain::{
    BlockerEntity, CandidateBoundary, CandidateDraft, CandidateEntity, CandidateRef,
    CoverageGoalEntity, CoverageResolutionEntity, CoverageResolutionKind, DraftIdentity, Error,
    EvidenceEntity, EvidenceKind, ResolvedCandidateDraft, Result, ScopeCandidateDraft,
};
use uuid::Uuid;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Kind {
    Goal,
    Evidence,
    Candidate,
    Blocker,
}

impl Kind {
    const fn noun(self) -> &'static str {
        match self {
            Self::Goal => "goal",
            Self::Evidence => "evidence",
            Self::Candidate => "candidate",
            Self::Blocker => "blocker",
        }
    }
}

fn reason(text: String) -> Error {
    Error::invalid_arguments_from(text)
}

pub(super) struct Source {
    pub(super) snapshot_id: Uuid,
    pub(super) kind: String,
    pub(super) input_sequence: Option<i64>,
    pub(super) program_field: Option<String>,
    pub(super) body: String,
}

pub(super) struct ResolveContext {
    pub(super) tenant_id: Uuid,
    pub(super) workspace_id: Uuid,
    pub(super) candidate_set_id: Uuid,
    pub(super) snapshot_id: Uuid,
    pub(super) latest_input: i64,
}

#[derive(sqlx::FromRow)]
struct SourceRow {
    id: Uuid,
    snapshot_id: Uuid,
    kind: String,
    input_sequence: Option<i64>,
    program_field: Option<String>,
    body: String,
}

pub(super) async fn resolve(
    transaction: &mut Transaction<'_, Postgres>,
    context: &ResolveContext,
    draft: &ScopeCandidateDraft,
    previous: Option<&ResolvedCandidateDraft>,
) -> Result<ResolvedCandidateDraft> {
    let rows = sqlx::query_as::<_, SourceRow>(
        "SELECT r.id,r.snapshot_id,r.kind,r.input_sequence,r.program_field,c.body \
         FROM scope_candidate_source_refs r \
         JOIN scope_candidate_contents c ON c.tenant_id=r.tenant_id \
          AND c.workspace_id=r.workspace_id AND c.digest=r.body_digest \
         WHERE r.tenant_id=$1 AND r.workspace_id=$2 AND r.candidate_set_id=$3",
    )
    .bind(context.tenant_id)
    .bind(context.workspace_id)
    .bind(context.candidate_set_id)
    .fetch_all(&mut **transaction)
    .await
    .map_err(storage_error)?;
    let sources: BTreeMap<_, _> = rows
        .into_iter()
        .map(|row| {
            (
                row.id,
                Source {
                    snapshot_id: row.snapshot_id,
                    kind: row.kind,
                    input_sequence: row.input_sequence,
                    program_field: row.program_field,
                    body: row.body,
                },
            )
        })
        .collect();
    let mut handles = BTreeMap::<String, (Kind, Uuid)>::new();
    allocate(
        &mut handles,
        Kind::Goal,
        draft.goals.iter().map(|v| &v.identity),
    )?;
    allocate(
        &mut handles,
        Kind::Evidence,
        draft.evidence.iter().map(|v| &v.identity),
    )?;
    allocate(
        &mut handles,
        Kind::Candidate,
        draft.candidates.iter().map(|v| &v.identity),
    )?;
    allocate(
        &mut handles,
        Kind::Blocker,
        draft.blockers.iter().map(|v| &v.identity),
    )?;
    let labels: super::links::Labels = handles
        .iter()
        .map(|(local, (_, id))| (*id, local.clone()))
        .collect();
    let old_goals = old_map(previous.map(|v| v.goals.as_slice()), |v| (v.id, v.revision));
    let old_evidence = old_map(previous.map(|v| v.evidence.as_slice()), |v| {
        (v.id, v.revision)
    });
    let old_candidates = old_map(previous.map(|v| v.candidates.as_slice()), |v| {
        (v.id, v.revision)
    });
    let old_blockers = old_map(previous.map(|v| v.blockers.as_slice()), |v| {
        (v.id, v.revision)
    });
    let goal_ids = entity_ids(&draft.goals, &handles, Kind::Goal, &old_goals, |v| {
        &v.identity
    })?;
    let evidence_ids = entity_ids(
        &draft.evidence,
        &handles,
        Kind::Evidence,
        &old_evidence,
        |v| &v.identity,
    )?;
    let candidate_ids = entity_ids(
        &draft.candidates,
        &handles,
        Kind::Candidate,
        &old_candidates,
        |v| &v.identity,
    )?;
    let blocker_ids = entity_ids(
        &draft.blockers,
        &handles,
        Kind::Blocker,
        &old_blockers,
        |v| &v.identity,
    )?;
    let mut goals = draft
        .goals
        .iter()
        .zip(&goal_ids)
        .enumerate()
        .map(|(index, (goal, (id, revision)))| {
            let source = sources.get(&goal.source_ref_id).ok_or_else(|| {
                reason(format!(
                    "goals[{index}].source_ref_id is not a source of this candidate set"
                ))
            })?;
            let expected = match draft.boundary {
                CandidateBoundary::Finite => "program_success",
                CandidateBoundary::Ongoing => "planning_input",
            };
            if source.snapshot_id != context.snapshot_id {
                return Err(reason(format!(
                    "goals[{index}] cites a source from another snapshot"
                )));
            }
            if source.kind != expected {
                return Err(reason(format!(
                    "goals[{index}] must cite the {expected} source, not {}",
                    source.kind
                )));
            }
            if source
                .input_sequence
                .is_some_and(|sequence| sequence > context.latest_input)
            {
                return Err(reason(format!(
                    "goals[{index}] cites input newer than this candidate set"
                )));
            }
            if goal
                .exact_quote
                .as_ref()
                .is_some_and(|quote| !source.body.contains(quote))
            {
                return Err(reason(format!(
                    "goals[{index}].exact_quote is not verbatim in its source; copy it exactly or omit it"
                )));
            }
            let kind = goal.resolution.kind;
            let target = match kind {
                CoverageResolutionKind::Candidate => resolve_ref(
                    &goal.resolution.reference,
                    &handles,
                    Kind::Candidate,
                    &candidate_ids,
                )?,
                CoverageResolutionKind::Evidence => resolve_ref(
                    &goal.resolution.reference,
                    &handles,
                    Kind::Evidence,
                    &evidence_ids,
                )?,
                CoverageResolutionKind::Blocker => resolve_ref(
                    &goal.resolution.reference,
                    &handles,
                    Kind::Blocker,
                    &blocker_ids,
                )?,
            };
            Ok(CoverageGoalEntity {
                id: *id,
                revision: *revision,
                text: goal.text.clone(),
                source_ref_id: goal.source_ref_id,
                exact_quote: goal.exact_quote.clone(),
                resolution: CoverageResolutionEntity { kind, id: target },
            })
        })
        .collect::<Result<Vec<_>>>()?;
    super::continuation::stabilize_goals(&mut goals, previous, &sources)?;
    let mut evidence = draft
        .evidence
        .iter()
        .zip(&evidence_ids)
        .enumerate()
        .map(|(index, (value, (id, revision)))| {
            let source = sources.get(&value.source_ref_id).ok_or_else(|| {
                reason(format!(
                    "evidence[{index}].source_ref_id is not a source of this candidate set"
                ))
            })?;
            if source.snapshot_id != context.snapshot_id {
                return Err(reason(format!(
                    "evidence[{index}] cites a source from another snapshot"
                )));
            }
            if value.authority_input_sequence.is_some_and(|sequence| {
                sequence < 1
                    || sequence > context.latest_input
                    || source.kind != "planning_input"
                    || source.input_sequence != Some(sequence)
            }) {
                return Err(reason(format!(
                    "evidence[{index}].authority_input_sequence must equal the input sequence of its planning_input source"
                )));
            }
            if value.kind == EvidenceKind::AcceptedWork && value.authority_input_sequence.is_none()
            {
                return Err(reason(format!(
                    "evidence[{index}] of kind accepted_work needs authority_input_sequence"
                )));
            }
            Ok(EvidenceEntity {
                id: *id,
                revision: *revision,
                kind: value.kind,
                summary: value.summary.clone(),
                source_ref_id: value.source_ref_id,
                authority_input_sequence: value.authority_input_sequence,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    super::continuation::stabilize_evidence(&mut evidence, previous, &sources)?;
    let mut blockers = draft
        .blockers
        .iter()
        .zip(&blocker_ids)
        .map(|(value, (id, revision))| {
            if sources
                .get(&value.source_ref_id)
                .is_none_or(|source| source.snapshot_id != context.snapshot_id)
            {
                return Err(reason(
                    "a blocker cites a source that is not in this snapshot".into(),
                ));
            }
            Ok(BlockerEntity {
                id: *id,
                revision: *revision,
                summary: value.summary.clone(),
                source_ref_id: value.source_ref_id,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    super::continuation::stabilize_blockers(&mut blockers, previous, &sources)?;
    let mut candidates = draft
        .candidates
        .iter()
        .zip(&candidate_ids)
        .map(|(value, (id, revision))| {
            candidate(
                value,
                *id,
                *revision,
                &handles,
                &candidate_ids,
                &goal_ids,
                &evidence_ids,
            )
        })
        .collect::<Result<Vec<_>>>()?;
    super::continuation::stabilize_candidates(&mut candidates, previous)?;
    super::links::validate_candidate_links(&goals, &candidates, &labels)?;
    let protected_changes = super::protected::resolve_protected_changes(
        previous,
        draft,
        &sources,
        &handles,
        &evidence_ids,
        &candidate_ids,
        &evidence,
        &candidates,
    )?;
    super::links::validate_acyclic(&candidates, &labels)?;
    if draft.empty_disposition.as_ref().is_some_and(|value| {
        sources
            .get(&value.source_ref_id)
            .is_none_or(|source| source.snapshot_id != context.snapshot_id)
    }) {
        return Err(reason(
            "empty_disposition cites a source that is not in this snapshot".into(),
        ));
    }
    let delta = super::continuation::candidate_delta(
        draft,
        previous,
        &candidates,
        &handles,
        &candidate_ids,
    )?;
    Ok(ResolvedCandidateDraft {
        boundary: draft.boundary,
        goals,
        evidence,
        candidates,
        blockers,
        pending_question: draft.pending_question.clone(),
        empty_disposition: draft.empty_disposition.clone(),
        protected_changes,
        delta,
    })
}

fn allocate<'a>(
    handles: &mut BTreeMap<String, (Kind, Uuid)>,
    kind: Kind,
    identities: impl Iterator<Item = &'a DraftIdentity>,
) -> Result<()> {
    for identity in identities {
        if let Some(local) = &identity.local
            && handles
                .insert(local.clone(), (kind, Uuid::new_v4()))
                .is_some()
        {
            return Err(reason(format!(
                "local label `{local}` is used by more than one draft entity"
            )));
        }
    }
    Ok(())
}

fn old_map<T>(values: Option<&[T]>, key: impl Fn(&T) -> (Uuid, i64)) -> BTreeMap<Uuid, i64> {
    values
        .unwrap_or_default()
        .iter()
        .map(key)
        .collect::<BTreeMap<_, _>>()
}

fn entity_ids<T>(
    values: &[T],
    handles: &BTreeMap<String, (Kind, Uuid)>,
    kind: Kind,
    old: &BTreeMap<Uuid, i64>,
    identity: impl Fn(&T) -> &DraftIdentity,
) -> Result<Vec<(Uuid, i64)>> {
    values
        .iter()
        .map(|value| {
            let identity = identity(value);
            if let Some(local) = &identity.local {
                let (actual_kind, id) = handles.get(local).ok_or(Error::InvalidArguments)?;
                return (*actual_kind == kind).then_some((*id, 1)).ok_or_else(|| {
                    reason(format!(
                        "local label `{local}` names a {}, not a {}",
                        actual_kind.noun(),
                        kind.noun()
                    ))
                });
            }
            let id = identity.id.ok_or_else(|| {
                reason(format!(
                    "a {} identity needs a local label or an id",
                    kind.noun()
                ))
            })?;
            let revision = old.get(&id).copied().ok_or_else(|| {
                reason(format!(
                    "{} id {id} is not in the previous revision; new entities use local labels",
                    kind.noun()
                ))
            })?;
            if identity.revision != Some(revision) {
                return Err(Error::StaleRevision);
            }
            Ok((id, revision))
        })
        .collect()
}

pub(super) fn resolve_ref(
    value: &CandidateRef,
    handles: &BTreeMap<String, (Kind, Uuid)>,
    kind: Kind,
    ids: &[(Uuid, i64)],
) -> Result<Uuid> {
    if let Some(local) = &value.local {
        let (actual, id) = handles.get(local).ok_or_else(|| {
            reason(format!(
                "reference `{local}` is not a local label declared in this draft"
            ))
        })?;
        return (*actual == kind).then_some(*id).ok_or_else(|| {
            reason(format!(
                "reference `{local}` names a {}, where a {} is required",
                actual.noun(),
                kind.noun()
            ))
        });
    }
    let id = value.id.ok_or_else(|| {
        reason(format!(
            "a {} reference needs a local label or an id",
            kind.noun()
        ))
    })?;
    ids.iter()
        .any(|(candidate, _)| *candidate == id)
        .then_some(id)
        .ok_or_else(|| {
            reason(format!(
                "reference {id} is not a {} of this draft",
                kind.noun()
            ))
        })
}

fn candidate(
    value: &CandidateDraft,
    id: Uuid,
    revision: i64,
    handles: &BTreeMap<String, (Kind, Uuid)>,
    candidates: &[(Uuid, i64)],
    goals: &[(Uuid, i64)],
    evidence: &[(Uuid, i64)],
) -> Result<CandidateEntity> {
    Ok(CandidateEntity {
        id,
        revision,
        title: value.title.clone(),
        outcome: value.outcome.clone(),
        trigger: value.trigger.clone(),
        delivered_behavior: value.delivered_behavior.clone(),
        proof: value.proof.clone(),
        includes: value.includes.clone(),
        excludes: value.excludes.clone(),
        dependencies: value
            .dependencies
            .iter()
            .map(|v| resolve_ref(v, handles, Kind::Candidate, candidates))
            .collect::<Result<_>>()?,
        coverage_goal_ids: value
            .coverage_goals
            .iter()
            .map(|v| resolve_ref(v, handles, Kind::Goal, goals))
            .collect::<Result<_>>()?,
        evidence_ids: value
            .evidence
            .iter()
            .map(|v| resolve_ref(v, handles, Kind::Evidence, evidence))
            .collect::<Result<_>>()?,
    })
}
