//! Jev advice inside commands: the workspace setting, the journal, one call per point.
use crate::{AdviceOutputGuard, AdviceStore, TransactionMode, WorkspaceService};
use tect_domain::*;
use uuid::Uuid;

pub(crate) mod words;

/// One advice point firing inside a command that has already saved its own work.
#[derive(Clone, Copy)]
pub struct AdviceRequest<'a> {
    pub point: &'a str,
    /// Route name of the agent's command.
    pub route: &'a str,
    pub work: &'a AdviceWorkRefs,
    pub ask: AdviceAsk<'a>,
    /// The command reply the field will be attached to.
    pub reply: &'a dyn AdviceOutputGuard,
}

/// What a point brings to the decision: its questions, or why it has none.
#[derive(Clone, Copy)]
pub enum AdviceAsk<'a> {
    Questions {
        questions: &'a AdviceQuestionSet,
        /// How the point turns accepted answers into its field, and its largest field.
        shaping: &'a dyn AdviceShaping,
    },
    /// A precondition of the point itself does not hold, so Jev is not asked.
    Unmet {
        reason: AdviceUnavailableReason,
        /// What is missing, as it reads after `Jev advice is not available: `.
        detail: &'a str,
    },
}

enum Opening {
    Disabled,
    Settled(AdviceField),
    Reserved(Uuid),
}

/// What is already decided about a point before it has prepared anything.
pub(crate) enum Earlier {
    /// Jev is off for the workspace: no field and no journal row.
    Disabled,
    /// The command was answered before under this request id.
    Stored(AdviceField),
    Undecided(AdviceConfig),
}

impl Earlier {
    /// The command's answer when the point is decided before anything is gathered: no
    /// field for a disabled Jev, the stored field for a replay. `None` means go on.
    pub(crate) fn answer(self) -> Option<Option<AdviceField>> {
        match self {
            Self::Disabled => Some(None),
            Self::Stored(advice) => Some(Some(advice)),
            Self::Undecided(_) => None,
        }
    }
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
    /// an error here: it is `unavailable` with a reason. So is a precondition of the
    /// point that does not hold: it is journalled and Jev is not asked.
    pub async fn advise(
        &self,
        context: &RequestContext,
        request: &AdviceRequest<'_>,
    ) -> Result<Option<AdviceField>> {
        validate_advice_point(request.point)?;
        validate_advice_route(request.route)?;
        request.work.validate()?;
        if let AdviceAsk::Questions { questions, .. } = request.ask {
            questions.validate()?;
        }

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
        let (entry_id, questions, shaping) = match (opening, request.ask) {
            (Opening::Disabled, _) => return Ok(None),
            (Opening::Settled(advice), _) => return Ok(Some(advice)),
            (Opening::Reserved(entry_id), AdviceAsk::Questions { questions, shaping }) => {
                (entry_id, questions, shaping)
            }
            // `open` reserves a call for questions only.
            (Opening::Reserved(_), AdviceAsk::Unmet { .. }) => {
                return Err(Error::InternalInvariant);
            }
        };

        let completion =
            AdviceCompletion::from_provider(self.advice_provider.ask(questions).await, shaping);

        let (mut tx, _) = self.authorized(context, TransactionMode::ReadWrite).await?;
        tx.complete_advice_entry(workspace.id, entry_id, &completion)
            .await?;
        tx.commit().await?;
        Ok(Some(completion.advice))
    }
}

/// The part of the decision that needs nothing from the point: a disabled Jev, and a
/// request id that already has a journal row. `lock` holds the setting row so that
/// what follows in the same transaction is one decision.
pub(crate) async fn earlier(
    store: &mut dyn AdviceStore,
    workspace_id: Uuid,
    lock: bool,
    (point, route): (&str, &str),
    request_id: Option<Uuid>,
) -> Result<Earlier> {
    let Some(config) = store
        .advice_config(workspace_id, lock)
        .await?
        .filter(|config| config.enabled)
    else {
        return Ok(Earlier::Disabled);
    };
    if let Some(request_id) = request_id
        && let Some(entry) = store
            .advice_entry_for_request(workspace_id, route, Some(point), request_id)
            .await?
    {
        return Ok(Earlier::Stored(entry.advice.unwrap_or_else(unanswered)));
    }
    Ok(Earlier::Undecided(config))
}

/// Decide, under the locked setting row, whether this point calls Jev, and journal it.
/// The order is fixed: disabled, replay, the point's own precondition, no provider,
/// reply fit, daily limit, call.
async fn open(
    store: &mut dyn AdviceStore,
    workspace_id: Uuid,
    session_id: Uuid,
    provider_configured: bool,
    request: &AdviceRequest<'_>,
) -> Result<Opening> {
    let names = (request.point, request.route);
    let config = match earlier(store, workspace_id, true, names, request.work.request_id).await? {
        Earlier::Disabled => return Ok(Opening::Disabled),
        Earlier::Stored(advice) => return Ok(Opening::Settled(advice)),
        Earlier::Undecided(config) => config,
    };
    // An enabled setting always carries a limit; the schema and the store both hold that.
    let limit = config.daily_call_limit.ok_or(Error::InternalInvariant)?;
    let skipped = match request.ask {
        AdviceAsk::Unmet { reason, detail } => Some(AdviceField::unavailable(reason, detail)),
        AdviceAsk::Questions { .. } if !provider_configured => Some(AdviceField::unavailable(
            AdviceUnavailableReason::ProviderUnconfigured,
            crate::advice_ports::NO_PROVIDER,
        )),
        AdviceAsk::Questions { shaping, .. } => {
            if let Some(too_large) = too_large_for_reply(shaping, request.reply)? {
                Some(too_large)
            } else if store.advice_calls_today(workspace_id).await? >= limit {
                Some(AdviceField::unavailable(
                    AdviceUnavailableReason::DailyLimitReached,
                    &format!("the workspace daily call limit of {limit} is reached"),
                ))
            } else {
                None
            }
        }
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

/// Jev is not asked for a field the command reply could not carry: the reply is
/// measured with the point's largest field before any call.
fn too_large_for_reply(
    shaping: &dyn AdviceShaping,
    reply: &dyn AdviceOutputGuard,
) -> Result<Option<AdviceField>> {
    match reply.check_reply(&shaping.largest()) {
        Ok(()) => Ok(None),
        Err(Error::RequestTooLarge) => Ok(Some(AdviceField::unavailable(
            AdviceUnavailableReason::AdviceTooLarge,
            &format!(
                "advice for {} would not fit the command reply",
                shaping.subject()
            ),
        ))),
        Err(error) => Err(error),
    }
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
