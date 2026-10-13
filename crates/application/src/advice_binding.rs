//! Jev's advice binds a ready review. Each use case that stores a review holds it to
//! the advice inside the review's own transaction, after the review is stored and
//! before it is committed: a refusal rolls the review back, and the departures a
//! passing review states are written with it.
use crate::AdviceStore;
use tect_domain::*;
use uuid::Uuid;

/// The review of one saved draft, as far as the advice on that draft is concerned.
pub(crate) struct Reviewed<'a> {
    pub level: SizingLevel,
    pub candidate_set_id: Uuid,
    /// The candidates of the draft under review with the revisions they stand under;
    /// `None` when the stored review came back without its draft.
    pub draft: Option<Vec<(Uuid, i64)>>,
    /// Whether the review says ready.
    pub ready: bool,
    pub departures: &'a [AdviceDeparture],
}

/// Hold a review to Jev's advice: refuse a ready one while advice that asks for a
/// change is open on its draft without a stated departure, and record the departures
/// a passing one states. With Jev disabled for the workspace nothing is read and
/// nothing is written: the review passes as it does on `main`, whatever advice was
/// left open from the time Jev was enabled.
pub(crate) async fn hold(
    store: &mut dyn AdviceStore,
    workspace_id: Uuid,
    reviewed: &Reviewed<'_>,
) -> Result<()> {
    let enabled = store
        .advice_config(workspace_id, false)
        .await?
        .is_some_and(|config| config.enabled);
    if !enabled {
        return Ok(());
    }
    // A review is of a draft. Its absence is held against the gate only, so that a
    // disabled Jev meets no error `main` does not have.
    let draft = reviewed.draft.as_deref().ok_or(Error::InternalInvariant)?;
    let open = store
        .open_advice(workspace_id, reviewed.level, reviewed.candidate_set_id)
        .await?;
    let departed =
        reviewed
            .level
            .review_departures(reviewed.ready, draft, &open, reviewed.departures)?;
    for (answer_id, reason) in departed {
        store
            .depart_from_advice(workspace_id, answer_id, reason)
            .await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
