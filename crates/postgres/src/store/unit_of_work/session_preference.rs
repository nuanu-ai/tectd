use super::*;

pub(super) async fn set(
    store: &mut PgUnitOfWork,
    workspace: Uuid,
    session: Uuid,
    request: &SetSessionAdvisoryPreference,
) -> Result<SessionAdvisoryPreference> {
    request.validate()?;
    let tenant = store.tenant_id()?;
    let identity = store.identity.as_ref().ok_or(Error::Forbidden)?;
    if !store.is_read_write() || identity.tenant_id != tenant {
        return Err(Error::Forbidden);
    }
    let host = identity.host_id;
    let principal = identity.principal_id;
    if store.session_principal(session).await? != principal {
        return Err(Error::Forbidden);
    }
    let current: Option<i64> = sqlx::query_scalar(
        "SELECT advisory_preference_revision FROM agent_sessions \
         WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3 AND host_id=$4 AND NOT revoked FOR UPDATE",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(session)
    .bind(host)
    .fetch_optional(&mut **store.transaction()?)
    .await
    .map_err(storage_error)?;
    let revision = current.ok_or(Error::Forbidden)?;
    let next = next_revision(revision, request)?;
    let changed = sqlx::query(
        "UPDATE agent_sessions SET advisory_preference=$5,advisory_preference_revision=$6 \
         WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3 AND host_id=$4 AND NOT revoked \
           AND advisory_preference_revision=$7",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(session)
    .bind(host)
    .bind(request.preference.as_str())
    .bind(next)
    .bind(revision)
    .execute(&mut **store.transaction()?)
    .await
    .map_err(storage_error)?;
    if changed.rows_affected() != 1 {
        return Err(Error::StaleRevision);
    }
    Ok(SessionAdvisoryPreference {
        preference: request.preference,
        revision: next,
    })
}

fn next_revision(revision: i64, request: &SetSessionAdvisoryPreference) -> Result<i64> {
    request.validate()?;
    if revision < 0 {
        return Err(Error::InternalInvariant);
    }
    if revision != request.expected_revision {
        return Err(Error::StaleRevision);
    }
    revision.checked_add(1).ok_or(Error::StorageUnavailable)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cas_rejects_stale_negative_and_overflow() {
        let mut request = SetSessionAdvisoryPreference {
            expected_revision: 0,
            preference: AdvisoryRequestPreference::Skip,
        };
        assert_eq!(next_revision(0, &request), Ok(1));
        assert_eq!(next_revision(1, &request), Err(Error::StaleRevision));
        request.expected_revision = -1;
        assert_eq!(next_revision(0, &request), Err(Error::InvalidArguments));
        request.expected_revision = i64::MAX;
        assert_eq!(
            next_revision(i64::MAX, &request),
            Err(Error::StorageUnavailable)
        );
    }
}

#[cfg(test)]
mod live_tests {
    use super::*;
    use crate::{PgStore, admin};
    use std::sync::Arc;
    use tect_application::WorkspaceService;

    fn service(store: Arc<PgStore>) -> WorkspaceService {
        WorkspaceService::new(
            store,
            Arc::new(tect_host::GitSourceInspector),
            Arc::new(tect_host::LocalSetupFiles),
        )
    }
    fn context(auth: &HostAuth) -> RequestContext {
        RequestContext {
            auth: auth.clone(),
            native_session_id: Uuid::new_v4().to_string(),
            workspace_key: format!("preference-{}", Uuid::new_v4()),
        }
    }
    async fn authorized(store: &PgStore, context: &RequestContext) -> Box<dyn UnitOfWork> {
        let mut tx = store.begin(TransactionMode::ReadWrite).await.unwrap();
        let identity = tx.authenticate(&context.auth).await.unwrap();
        tx.set_tenant(identity.tenant_id).await.unwrap();
        tx.lock_native_session(identity.host_id, &context.native_session_id)
            .await
            .unwrap();
        tx
    }

    #[tokio::test]
    #[ignore = "requires disposable PG18 and explicit TECT_TEST_ADMIN_URL/TECT_TEST_RUNTIME_URL/TECT_TEST_RUNTIME_ROLE"]
    async fn native_preference_default_cas_isolation_and_same_lock_ordering() {
        let admin_url = std::env::var("TECT_TEST_ADMIN_URL").unwrap();
        let runtime_url = std::env::var("TECT_TEST_RUNTIME_URL").unwrap();
        let role = std::env::var("TECT_TEST_RUNTIME_ROLE").unwrap();
        assert_ne!(admin_url, runtime_url);
        let pool = sqlx::PgPool::connect(&admin_url).await.unwrap();
        admin::migrate(&pool, &role).await.unwrap();
        admin::migrate(&pool, &role).await.unwrap();
        admin::validate_runtime_role(&pool, &role).await.unwrap();
        let preference_acl: (bool, bool, bool, bool) = sqlx::query_as(
            "SELECT pg_catalog.has_column_privilege($1,'public.agent_sessions', \
                 'advisory_preference','UPDATE'), \
             pg_catalog.has_column_privilege($1,'public.agent_sessions', \
                 'advisory_preference_revision','UPDATE'), \
             pg_catalog.has_table_privilege($1,'public.agent_sessions','UPDATE'), \
             EXISTS(SELECT 1 FROM pg_catalog.pg_attribute a \
                 WHERE a.attrelid='public.agent_sessions'::regclass \
                   AND a.attnum>0 AND NOT a.attisdropped \
                   AND a.attname NOT IN ('advisory_preference','advisory_preference_revision') \
                   AND pg_catalog.has_column_privilege($1,a.attrelid,a.attnum,'UPDATE'))",
        )
        .bind(&role)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(preference_acl, (true, true, false, false));
        let owner = admin::enroll_host(&pool, None, vec![]).await.unwrap();
        let other = admin::enroll_host(&pool, None, vec![]).await.unwrap();
        let store = Arc::new(PgStore::connect(&runtime_url, 4).await.unwrap());
        let app = service(Arc::clone(&store));
        let context = context(&owner.auth);
        let foreign = self::context(&other.auth);
        let opened = app.open_workspace(&context).await.unwrap();
        let foreign_opened = app.open_workspace(&foreign).await.unwrap();
        let workspace = opened.workspace.unwrap().id;
        let session = opened.session.unwrap().id;
        let foreign_workspace = foreign_opened.workspace.unwrap().id;
        let foreign_session = foreign_opened.session.unwrap().id;
        let tenant: Uuid = sqlx::query_scalar("SELECT tenant_id FROM agent_sessions WHERE id=$1")
            .bind(session)
            .fetch_one(&pool)
            .await
            .unwrap();
        let runtime_pool = sqlx::PgPool::connect(&runtime_url).await.unwrap();
        let mut denied = runtime_pool.begin().await.unwrap();
        let runtime_role: String = sqlx::query_scalar("SELECT current_user::text")
            .fetch_one(&mut *denied)
            .await
            .unwrap();
        assert_eq!(runtime_role, role);
        sqlx::query("SELECT pg_catalog.set_config('tect.tenant_id',$1,true)")
            .bind(tenant.to_string())
            .execute(&mut *denied)
            .await
            .unwrap();
        let visible: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM agent_sessions WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3",
        )
        .bind(tenant)
        .bind(workspace)
        .bind(session)
        .fetch_one(&mut *denied)
        .await
        .unwrap();
        assert_eq!(visible, 1);
        let forbidden = sqlx::query(
            "UPDATE agent_sessions SET revoked=revoked WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3",
        )
        .bind(tenant)
        .bind(workspace)
        .bind(session)
        .execute(&mut *denied)
        .await
        .unwrap_err();
        assert_eq!(
            forbidden.as_database_error().unwrap().code().as_deref(),
            Some("42501")
        );
        denied.rollback().await.unwrap();
        assert_eq!(
            app.session_advisory_preference(&context).await.unwrap(),
            SessionAdvisoryPreference {
                preference: AdvisoryRequestPreference::UseWorkspace,
                revision: 0
            }
        );
        let mut guarded = authorized(&store, &context).await;
        assert_eq!(
            guarded
                .session_advisory_preference(workspace, Uuid::new_v4())
                .await,
            Err(Error::Forbidden)
        );
        assert_eq!(
            guarded
                .session_advisory_preference(foreign_workspace, foreign_session)
                .await,
            Err(Error::Forbidden)
        );
        assert_eq!(
            guarded
                .set_session_advisory_preference(
                    foreign_workspace,
                    foreign_session,
                    &SetSessionAdvisoryPreference {
                        expected_revision: 0,
                        preference: AdvisoryRequestPreference::Skip
                    }
                )
                .await,
            Err(Error::Forbidden)
        );
        guarded.commit().await.unwrap();

        // Commit Skip while holding the very lock used by Scope authorization.
        let mut setter = authorized(&store, &context).await;
        let skipped = setter
            .set_session_advisory_preference(
                workspace,
                session,
                &SetSessionAdvisoryPreference {
                    expected_revision: 0,
                    preference: AdvisoryRequestPreference::Skip,
                },
            )
            .await
            .unwrap();
        assert_eq!(skipped.revision, 1);
        let waiting_store = Arc::clone(&store);
        let waiting_context = context.clone();
        let (attempting, attempted) = tokio::sync::oneshot::channel();
        let mut waiter = tokio::spawn(async move {
            attempting.send(()).unwrap();
            let mut authorization = authorized(&waiting_store, &waiting_context).await;
            let value = authorization
                .session_advisory_preference(workspace, session)
                .await
                .unwrap();
            authorization.commit().await.unwrap();
            value
        });
        attempted.await.unwrap();
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(100), &mut waiter)
                .await
                .is_err()
        );
        setter.commit().await.unwrap();
        assert_eq!(waiter.await.unwrap(), skipped);
        assert_eq!(
            app.set_session_advisory_preference(
                &context,
                &SetSessionAdvisoryPreference {
                    expected_revision: 0,
                    preference: AdvisoryRequestPreference::UseWorkspace
                }
            )
            .await,
            Err(Error::StaleRevision)
        );
        // Concurrent equal revisions: at most one update commits.
        let request = SetSessionAdvisoryPreference {
            expected_revision: 1,
            preference: AdvisoryRequestPreference::UseWorkspace,
        };
        let (a, b) = tokio::join!(
            app.set_session_advisory_preference(&context, &request),
            app.set_session_advisory_preference(&context, &request)
        );
        assert!(matches!(
            (&a, &b),
            (Ok(_), Err(Error::StaleRevision)) | (Err(Error::StaleRevision), Ok(_))
        ));
        assert_eq!(
            app.session_advisory_preference(&context)
                .await
                .unwrap()
                .revision,
            2
        );
        let mut unauthenticated = context.clone();
        unauthenticated.auth.credential.push('x');
        assert!(
            app.session_advisory_preference(&unauthenticated)
                .await
                .is_err()
        );
        admin::revoke_session(&pool, session).await.unwrap();
        assert!(matches!(
            app.session_advisory_preference(&context).await,
            Err(Error::SessionRevoked)
        ));
    }
}
