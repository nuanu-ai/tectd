use super::*;

pub(super) struct ReviewPending {
    pub resource: PipelineKnowledgeResource,
    pub purpose: KnowledgeBindingPurpose,
    pub review_due: bool,
    pub dk2_event: Option<Uuid>,
    pub gap_index: usize,
    pub warning_index: usize,
    pub boundary_index: usize,
}

pub(super) struct ReviewVectors<'a> {
    pub selected: &'a mut Vec<PipelineKnowledgeResource>,
    pub gaps: &'a mut Vec<String>,
    pub warnings: &'a mut Vec<String>,
    pub temporal_boundaries: &'a mut Vec<String>,
}

pub(super) async fn finalize(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    principal: Uuid,
    pending_reviews: Vec<ReviewPending>,
    vectors: ReviewVectors<'_>,
) -> Result<bool> {
    // A second read is intentional under READ COMMITTED: review status has its
    // own fresh publication proof, independent of the earlier resource proof.
    let mut review_keys = Vec::new();
    let mut review_indices = HashMap::new();
    for pending in &pending_reviews {
        if let Some(event) = pending.dk2_event {
            let key = (pending.resource.unit_id, pending.resource.revision, event);
            if let std::collections::hash_map::Entry::Vacant(entry) = review_indices.entry(key) {
                entry.insert(review_keys.len());
                review_keys.push(key);
            }
        }
    }
    let review_rows = crate::knowledge_lifecycle::rdf::canonical_native_rows(
        tx,
        tenant,
        workspace,
        principal,
        &review_keys,
    )
    .await?;
    if review_rows.len() != review_keys.len() {
        return Err(Error::InternalInvariant);
    }
    let mut reviews = Vec::with_capacity(pending_reviews.len());
    for pending in &pending_reviews {
        let review = if let Some(event) = pending.dk2_event {
            let key = (pending.resource.unit_id, pending.resource.revision, event);
            let index = review_indices.get(&key).ok_or(Error::InternalInvariant)?;
            crate::knowledge_maintenance::current_unit_review_status_with_rows(
                tx,
                tenant,
                workspace,
                principal,
                pending.resource.unit_id,
                pending.resource.revision,
                event,
                &review_rows[*index],
            )
            .await?
        } else {
            crate::knowledge_maintenance::current_unit_review_status(
                tx,
                tenant,
                workspace,
                principal,
                pending.resource.unit_id,
                pending.resource.revision,
            )
            .await?
        };
        reviews.push(review);
    }
    // Insert deferred results from the back to preserve the original binding
    // walk's gap, warning, temporal-boundary, and selected-resource ordering.
    let mut has_dk2_selected = false;
    for (pending, review) in pending_reviews.into_iter().zip(reviews).rev() {
        let boundaries = [review.valid_from, review.valid_until, review.review_due_at]
            .into_iter()
            .flatten();
        vectors
            .temporal_boundaries
            .splice(pending.boundary_index..pending.boundary_index, boundaries);
        let mut review_warnings = Vec::new();
        if review.needs_review {
            if blocking(pending.purpose) {
                vectors.gaps.insert(
                    pending.gap_index,
                    format!("knowledge_needs_review:{}", pending.resource.unit_id),
                );
            } else {
                review_warnings.push(format!(
                    "optional_knowledge_needs_review:{}",
                    pending.resource.unit_id
                ));
            }
        }
        if pending.review_due {
            review_warnings.push(format!("review_due:{}", pending.resource.unit_id));
        }
        vectors.warnings.splice(
            pending.warning_index..pending.warning_index,
            review_warnings,
        );
        has_dk2_selected |= pending.dk2_event.is_some();
        vectors.selected.push(pending.resource);
    }
    Ok(has_dk2_selected)
}
