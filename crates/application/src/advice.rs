//! Jev advice inside commands: the workspace setting, the journal, one call per point.
use crate::{AdviceStore, TransactionMode, WorkspaceService};
use tect_domain::*;
use uuid::Uuid;

/// One advice point firing inside a command that has already saved its own work.
#[derive(Debug, Clone, Copy)]
pub struct AdviceRequest<'a> {
    pub point: &'a str,
    /// Route name of the agent's command.
    pub route: &'a str,
    pub work: &'a AdviceWorkRefs,
    pub questions: &'a AdviceQuestionSet,
}

enum Opening {
    Disabled,
    Settled(AdviceField),
    Reserved(Uuid),
}

impl WorkspaceService {
    pub async fn advice_config(&self, context: &RequestContext) -> Result<AdviceConfigStatus> {
        let (mut tx, identity) = self.authorized(context, TransactionMode::ReadOnly).await?;
        let (workspace, _) = Self::bound_session(&mut *tx, context, &identity).await?;
        let status = AdviceConfigStatus {
            config: tx.advice_config(workspace.id, false).await?,
            calls_today: tx.advice_calls_today(workspace.id).await?,
            provider_configured: self.advice_provider.configured(),
        };
        tx.commit().await?;
        Ok(status)
    }

    pub async fn configure_advice(
        &self,
        context: &RequestContext,
        request: &ConfigureAdvice,
    ) -> Result<AdviceConfigStatus> {
        request.validate()?;
        let (mut tx, identity) = self.authorized(context, TransactionMode::ReadWrite).await?;
        tx.lock_native_session(identity.host_id, &context.native_session_id)
            .await?;
        let (workspace, session) = Self::bound_session(&mut *tx, context, &identity).await?;
        let config = tx
            .configure_advice(workspace.id, session.id, request)
            .await?;
        let status = AdviceConfigStatus {
            config: Some(config),
            calls_today: tx.advice_calls_today(workspace.id).await?,
            provider_configured: self.advice_provider.configured(),
        };
        tx.commit().await?;
        Ok(status)
    }

    pub async fn advice_audit(
        &self,
        context: &RequestContext,
        query: &AdviceAuditQuery,
    ) -> Result<AdviceAuditPage> {
        query.validate()?;
        let (mut tx, identity) = self.authorized(context, TransactionMode::ReadOnly).await?;
        let (workspace, _) = Self::bound_session(&mut *tx, context, &identity).await?;
        let counters = tx.advice_counters(workspace.id, query).await?;
        let mut entries = if query.limit == 0 {
            Vec::new()
        } else {
            tx.advice_entries(workspace.id, query, query.limit + 1)
                .await?
        };
        let has_more = entries.len() > query.limit as usize;
        entries.truncate(query.limit as usize);
        let next_after = if has_more {
            entries.last().map(|entry| entry.id)
        } else {
            None
        };
        tx.commit().await?;
        Ok(AdviceAuditPage {
            counters,
            entries,
            next_after,
        })
    }

    /// The reply field a command stored under `request_id`, newest row first, for an
    /// idempotent replay. `None` when Jev is disabled or nothing is stored.
    pub async fn stored_advice(
        &self,
        context: &RequestContext,
        route: &str,
        request_id: Uuid,
    ) -> Result<Option<AdviceField>> {
        validate_advice_route(route)?;
        if request_id.is_nil() {
            return Err(Error::InvalidArguments);
        }
        let (mut tx, identity) = self.authorized(context, TransactionMode::ReadOnly).await?;
        let (workspace, _) = Self::bound_session(&mut *tx, context, &identity).await?;
        let enabled = tx
            .advice_config(workspace.id, false)
            .await?
            .is_some_and(|config| config.enabled);
        let stored = if enabled {
            tx.advice_entry_for_request(workspace.id, route, None, request_id)
                .await?
                .and_then(|entry| entry.advice)
        } else {
            None
        };
        tx.commit().await?;
        Ok(stored)
    }

    /// Ask Jev once for one advice point and journal it.
    ///
    /// Returns `None`, writing nothing, when Jev is disabled for the workspace. Otherwise
    /// the value is the `advice` field of the command reply. A journal row is written
    /// before the single provider call and completed after it; the call itself runs
    /// outside any database transaction and is never retried. A provider failure is not
    /// an error here: it is `unavailable` with a reason.
    pub async fn advise(
        &self,
        context: &RequestContext,
        request: &AdviceRequest<'_>,
    ) -> Result<Option<AdviceField>> {
        validate_advice_point(request.point)?;
        validate_advice_route(request.route)?;
        request.work.validate()?;
        request.questions.validate()?;

        let (mut tx, identity) = self.authorized(context, TransactionMode::ReadWrite).await?;
        let (workspace, session) = Self::bound_session(&mut *tx, context, &identity).await?;
        let opening = open(
            &mut *tx,
            workspace.id,
            session.id,
            self.advice_provider.configured(),
            request,
        )
        .await?;
        tx.commit().await?;
        let entry_id = match opening {
            Opening::Disabled => return Ok(None),
            Opening::Settled(advice) => return Ok(Some(advice)),
            Opening::Reserved(entry_id) => entry_id,
        };

        let completion =
            AdviceCompletion::from_provider(self.advice_provider.ask(request.questions).await);

        let (mut tx, _) = self.authorized(context, TransactionMode::ReadWrite).await?;
        tx.complete_advice_entry(workspace.id, entry_id, &completion)
            .await?;
        tx.commit().await?;
        Ok(Some(completion.advice))
    }
}

/// Decide, under the locked setting row, whether this point calls Jev, and journal it.
async fn open(
    store: &mut dyn AdviceStore,
    workspace_id: Uuid,
    session_id: Uuid,
    provider_configured: bool,
    request: &AdviceRequest<'_>,
) -> Result<Opening> {
    let Some(config) = store
        .advice_config(workspace_id, true)
        .await?
        .filter(|config| config.enabled)
    else {
        return Ok(Opening::Disabled);
    };
    if let Some(request_id) = request.work.request_id
        && let Some(entry) = store
            .advice_entry_for_request(workspace_id, request.route, Some(request.point), request_id)
            .await?
    {
        return Ok(Opening::Settled(entry.advice.unwrap_or_else(unanswered)));
    }
    // An enabled setting always carries a limit; the schema and the store both hold that.
    let limit = config.daily_call_limit.ok_or(Error::InternalInvariant)?;
    let skipped = if !provider_configured {
        Some(AdviceField::unavailable(
            AdviceUnavailableReason::ProviderUnconfigured,
            crate::advice_ports::NO_PROVIDER,
        ))
    } else if store.advice_calls_today(workspace_id).await? >= limit {
        Some(AdviceField::unavailable(
            AdviceUnavailableReason::DailyLimitReached,
            &format!("the workspace daily call limit of {limit} is reached"),
        ))
    } else {
        None
    };
    let entry = NewAdviceEntry {
        point: request.point.to_owned(),
        route: request.route.to_owned(),
        work: request.work.clone(),
        not_called: skipped.clone(),
    };
    let entry_id = store
        .insert_advice_entry(workspace_id, session_id, &entry)
        .await?;
    Ok(match skipped {
        Some(advice) => Opening::Settled(advice),
        None => Opening::Reserved(entry_id),
    })
}

/// A replay of a command whose earlier call was sent and never completed.
fn unanswered() -> AdviceField {
    AdviceField::unavailable(
        AdviceUnavailableReason::TransportError,
        "an earlier attempt for this request left no recorded answer",
    )
}

#[cfg(test)]
mod tests;
