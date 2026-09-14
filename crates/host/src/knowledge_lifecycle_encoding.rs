use crate::{knowledge_lifecycle_output, responses};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tect_application::KnowledgeOutputGuard;
use tect_domain::*;
use uuid::Uuid;

pub(crate) struct KnowledgeEncoding {
    capacity: usize,
    query: Option<EncodingQuery>,
}

enum EncodingQuery {
    Lifecycle(KnowledgeLifecycleQuery),
    Unit(KnowledgeUnitQuery),
}

impl KnowledgeEncoding {
    pub(crate) const fn new(capacity: usize) -> Self {
        Self {
            capacity,
            query: None,
        }
    }

    pub(crate) const fn lifecycle(capacity: usize, query: KnowledgeLifecycleQuery) -> Self {
        Self {
            capacity,
            query: Some(EncodingQuery::Lifecycle(query)),
        }
    }

    pub(crate) const fn unit(capacity: usize, query: KnowledgeUnitQuery) -> Self {
        Self {
            capacity,
            query: Some(EncodingQuery::Unit(query)),
        }
    }
}

impl KnowledgeOutputGuard for KnowledgeEncoding {
    fn lifecycle(&self, value: &KnowledgeLifecycleResponse) -> Result<()> {
        let Some(EncodingQuery::Lifecycle(query)) = self.query.as_ref() else {
            return Err(Error::InternalInvariant);
        };
        knowledge_lifecycle_output::lifecycle(value.clone(), query, self.capacity).map(drop)
    }

    fn unit(&self, value: &KnowledgeUnitResponse) -> Result<()> {
        let Some(EncodingQuery::Unit(query)) = self.query.as_ref() else {
            return Err(Error::InternalInvariant);
        };
        knowledge_lifecycle_output::unit(value.clone(), query, self.capacity).map(drop)
    }

    fn begin(&self, value: &BeginKnowledgeChangeOutcome) -> Result<()> {
        knowledge_lifecycle_output::begin(value.clone(), self.capacity).map(drop)
    }

    fn mutation(&self, value: &KnowledgeChangeMutationOutcome) -> Result<()> {
        knowledge_lifecycle_output::mutation(value.clone(), self.capacity).map(drop)
    }

    fn commit(&self, value: &CommitKnowledgeChangeOutcome) -> Result<()> {
        knowledge_lifecycle_output::commit(value.clone(), self.capacity).map(drop)
    }

    fn effects(&self, value: &SettleKnowledgeChangeEffectsOutcome) -> Result<()> {
        knowledge_lifecycle_output::settle(Uuid::max(), value.clone(), self.capacity).map(drop)
    }
}

pub(crate) fn fragment(
    value: Value,
    query: &KnowledgeLifecycleQuery,
    capacity: usize,
) -> Result<Value> {
    fragment_value_for_query(
        value,
        serde_json::to_value(query).map_err(|_| Error::TransportUnavailable)?,
        query.fragment.as_ref(),
        "knowledge_lifecycle",
        capacity,
    )
}

pub(crate) fn unit_fragment(
    value: Value,
    query: &KnowledgeUnitQuery,
    capacity: usize,
) -> Result<Value> {
    fragment_value_for_query(
        value,
        serde_json::to_value(query).map_err(|_| Error::TransportUnavailable)?,
        query.fragment.as_ref(),
        "knowledge_unit",
        capacity,
    )
}

fn fragment_value_for_query(
    value: Value,
    mut params: Value,
    requested: Option<&KnowledgeLifecycleFragmentQuery>,
    route: &str,
    capacity: usize,
) -> Result<Value> {
    let bytes = serde_json::to_vec(&value).map_err(|_| Error::TransportUnavailable)?;
    let text = std::str::from_utf8(&bytes).map_err(|_| Error::InternalInvariant)?;
    let snapshot_digest = sha256(&bytes);
    if requested.is_some_and(|fragment| fragment.offset > 0 && fragment.snapshot_digest.is_none()) {
        return Err(Error::InvalidArguments);
    }
    if requested
        .and_then(|fragment| fragment.snapshot_digest.as_deref())
        .is_some_and(|digest| digest != snapshot_digest)
    {
        return Err(Error::StaleContext);
    }
    let offset = usize::try_from(requested.map_or(0, |fragment| fragment.offset))
        .map_err(|_| Error::InvalidArguments)?;
    if offset > bytes.len() || !text.is_char_boundary(offset) {
        return Err(Error::InvalidArguments);
    }
    let limit = usize::try_from(requested.map_or(262_144, |fragment| fragment.limit))
        .map_err(|_| Error::InvalidArguments)?;
    let mut upper = offset.saturating_add(limit).min(bytes.len());
    while upper < bytes.len() && !text.is_char_boundary(upper) {
        upper += 1;
    }
    let boundaries: Vec<usize> = text[offset..upper]
        .char_indices()
        .map(|(index, _)| offset + index)
        .chain(std::iter::once(upper))
        .filter(|end| *end > offset || offset == text.len())
        .collect();
    let mut low = 0;
    let mut high = boundaries.len();
    let mut answer = None;
    while low < high {
        let middle = (low + high) / 2;
        let end = boundaries[middle];
        let candidate = fragment_value(
            text,
            &mut params,
            route,
            &snapshot_digest,
            offset,
            end,
            limit,
        )?;
        if responses::encoded_len(&candidate)? <= capacity {
            answer = Some(candidate);
            low = middle + 1;
        } else {
            high = middle;
        }
    }
    answer.ok_or(Error::RequestTooLarge)
}

fn fragment_value(
    text: &str,
    params: &mut Value,
    route: &str,
    digest: &str,
    offset: usize,
    end: usize,
    limit: usize,
) -> Result<Value> {
    let mut actions = Vec::new();
    if end < text.len() {
        params["fragment"] = json!({"snapshot_digest":digest,"offset":end,"limit":limit});
        actions.push(responses::action(route, params.clone())?);
    }
    Ok(responses::with_actions(
        json!({"fragment":{"encoding":"utf8","snapshot_digest":digest,"offset":offset,
            "byte_length":end-offset,"total_bytes":text.len(),"text":&text[offset..end]}}),
        actions,
        if end < text.len() { Some(0) } else { None },
    ))
}

fn sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lifecycle() -> KnowledgeLifecycleResponse {
        KnowledgeLifecycleResponse::Overview(KnowledgeLifecycleOverview {
            workspace_generation: 7,
            active: (0..40)
                .map(|index| KnowledgeLifecycleSummary {
                    change_id: Uuid::new_v4(),
                    run_id: Uuid::new_v4(),
                    status: PipelineRunStatus::Active,
                    current_phase_id: Some(KnowledgeChangePhaseId::KcQualifyEvidence),
                    operation_count: index,
                })
                .collect(),
        })
    }

    #[test]
    fn utf8_fragments_reconstruct_the_exact_typed_snapshot() {
        let value = responses::with_actions(
            json!(lifecycle()),
            vec![responses::action("knowledge_lifecycle", json!({})).unwrap()],
            Some(0),
        );
        let expected = serde_json::to_string(&value).unwrap();
        let mut query = KnowledgeLifecycleQuery {
            change_id: None,
            view: KnowledgeLifecycleView::Current,
            output_id: None,
            digest: None,
            fragment: Some(KnowledgeLifecycleFragmentQuery {
                snapshot_digest: None,
                offset: 0,
                limit: 5_000,
            }),
        };
        let mut restored = String::new();
        let mut digest = None;
        loop {
            let page = fragment(value.clone(), &query, 1_600).unwrap();
            assert!(responses::encoded_len(&page).unwrap() <= 1_600);
            let current = page["fragment"]["snapshot_digest"].as_str().unwrap();
            assert!(digest.as_deref().is_none_or(|prior| prior == current));
            digest = Some(current.to_owned());
            restored.push_str(page["fragment"]["text"].as_str().unwrap());
            let Some(action) = page["actions"].as_array().and_then(|items| items.first()) else {
                break;
            };
            query = serde_json::from_value(action["arguments"]["params"].clone()).unwrap();
        }
        assert_eq!(restored, expected);
        let reconstructed: Value = serde_json::from_str(&restored).unwrap();
        assert_eq!(reconstructed["actions"], value["actions"]);
    }

    #[test]
    fn changed_snapshot_rejects_a_pinned_fragment_cursor() {
        let value = json!(lifecycle());
        let query = KnowledgeLifecycleQuery {
            change_id: None,
            view: KnowledgeLifecycleView::Current,
            output_id: None,
            digest: None,
            fragment: Some(KnowledgeLifecycleFragmentQuery {
                snapshot_digest: Some("stale".into()),
                offset: 0,
                limit: 1_000,
            }),
        };
        assert_eq!(fragment(value, &query, 1_600), Err(Error::StaleContext));
    }

    #[test]
    fn arbitrary_initial_limits_do_not_split_cyrillic_or_return_empty_progress() {
        let value = json!({"text":"Жизненный цикл знания ".repeat(20)});
        let serialized = serde_json::to_string(&value).unwrap();
        let offset = serialized.find('Ж').unwrap();
        let digest = sha256(serialized.as_bytes());
        for limit in 1..=128 {
            let query = KnowledgeLifecycleQuery {
                change_id: None,
                view: KnowledgeLifecycleView::Current,
                output_id: None,
                digest: None,
                fragment: Some(KnowledgeLifecycleFragmentQuery {
                    snapshot_digest: Some(digest.clone()),
                    offset: offset as u64,
                    limit,
                }),
            };
            let page = fragment(value.clone(), &query, 8_000).unwrap();
            assert!(page["fragment"]["byte_length"].as_u64().unwrap() > 0);
            assert!(page["fragment"]["text"].as_str().is_some());
        }
    }

    #[test]
    fn continuation_requires_a_snapshot_digest() {
        let query = KnowledgeLifecycleQuery {
            change_id: None,
            view: KnowledgeLifecycleView::Current,
            output_id: None,
            digest: None,
            fragment: Some(KnowledgeLifecycleFragmentQuery {
                snapshot_digest: None,
                offset: 1,
                limit: 20,
            }),
        };
        assert_eq!(
            fragment(json!({"value":"test"}), &query, 2_000),
            Err(Error::InvalidArguments)
        );
    }

    #[test]
    fn unit_fragment_continuation_preserves_the_exact_unit_query() {
        let unit_id = Uuid::new_v4();
        let query = KnowledgeUnitQuery {
            unit_id,
            revision: Some(7),
            fragment: Some(KnowledgeLifecycleFragmentQuery {
                snapshot_digest: None,
                offset: 0,
                limit: 10_000,
            }),
        };
        let value = json!({"document":"bounded ".repeat(2_000)});
        let page = unit_fragment(value, &query, 1_600).unwrap();
        assert_eq!(page["actions"][0]["arguments"]["route"], "knowledge.unit");
        assert_eq!(
            page["actions"][0]["arguments"]["params"]["unit_id"],
            json!(unit_id)
        );
        assert_eq!(page["actions"][0]["arguments"]["params"]["revision"], 7);
        assert!(
            page["actions"][0]["arguments"]["params"]["fragment"]["snapshot_digest"].is_string()
        );
    }
}
