//! Migration 0045 on a database that already holds a workspace with a Jev setting,
//! journal rows, drafts and reviews: all of it is left as it was, and the new table
//! starts empty, forced under row security, with the runtime role's narrow grants.
use crate::upgrade::{Rows, in_own_database, migrate_before, seed};
use serde_json::{Value, json};
use sqlx::{PgPool, Row};
use tect_domain::{ScopeCandidateReview, SliceCandidateReview};
use tect_postgres::admin;
use uuid::Uuid;

const TABLE: &str = "advice_candidate_answers";

/// What the workspace held from the time Jev advised without binding: its setting,
/// two journal rows and a ready review at each level. Returns the advised journal row.
async fn seed_advice(pool: &PgPool, rows: &Rows) -> Uuid {
    let [principal, host, session, journal] = [(); 4].map(|()| Uuid::new_v4());
    let run = async |sql: &str, ids: &[Uuid], payload: Option<&Value>| {
        let mut query = sqlx::query(sql);
        for id in ids {
            query = query.bind(*id);
        }
        if let Some(payload) = payload {
            query = query.bind(payload);
        }
        query.execute(pool).await.unwrap();
    };
    run(
        "INSERT INTO principals(id,tenant_id,role) VALUES($1,$2,'owner')",
        &[principal, rows.tenant],
        None,
    )
    .await;
    run(
        "INSERT INTO hosts(id,tenant_id,principal_id,credential_digest) \
         VALUES($1,$2,$3,pg_catalog.repeat('e',64))",
        &[host, rows.tenant, principal],
        None,
    )
    .await;
    run(
        "INSERT INTO agent_sessions(id,tenant_id,host_id,workspace_id,native_session_id) \
         VALUES($1,$2,$3,$4,$1::text)",
        &[session, rows.tenant, host, rows.workspace],
        None,
    )
    .await;
    run(
        "INSERT INTO advice_config(tenant_id,workspace_id,enabled,daily_call_limit,revision,\
         updated_by_session) VALUES($1,$2,true,20,1,$3)",
        &[rows.tenant, rows.workspace, session],
        None,
    )
    .await;
    let advice = json!({"status":"advised","note":"an earlier note","level":"program_to_scopes",
        "candidates":[{"candidate":1,"size":"large","size_p":0.6,"advice":"split",
            "advice_p":0.8,"extra_items":[1],"extra_p":[0.7]}]});
    run(
        "INSERT INTO advice_journal(id,tenant_id,workspace_id,session_id,point,route,request_id,\
         candidate_set_id,called,outcome,send_certainty,advice) VALUES($1,$2,$3,$4,'sizing',\
         'scope.candidates.save',$5,$6,true,'advised','sent',$7)",
        &[
            journal,
            rows.tenant,
            rows.workspace,
            session,
            Uuid::new_v4(),
            rows.scope_set,
        ],
        Some(&advice),
    )
    .await;
    let unavailable = json!({"status":"unavailable","reason":"timeout",
        "message":"Jev advice is not available: Jev did not answer within 800 ms"});
    run(
        "INSERT INTO advice_journal(tenant_id,workspace_id,session_id,point,route,request_id,\
         candidate_set_id,scope_id,called,outcome,reason,send_certainty,advice) \
         VALUES($1,$2,$3,'pipeline','slice.candidates.save',$4,$5,$6,true,'unavailable',\
         'timeout','sent_unknown',$7)",
        &[
            rows.tenant,
            rows.workspace,
            session,
            Uuid::new_v4(),
            rows.slice_set,
            rows.scope,
        ],
        Some(&unavailable),
    )
    .await;
    // The reviews that made the two seeded sets ready, as `main` stores a review.
    let scope_review = json!({"revision":3,"verdict":"ready","summary":"Bounded and ready",
        "findings":[],"candidate_decisions":[],"protected_change_reviews":[]});
    run(
        "INSERT INTO scope_candidate_reviews(tenant_id,workspace_id,candidate_set_id,\
         set_revision,payload) VALUES($1,$2,$3,3,$4)",
        &[rows.tenant, rows.workspace, rows.scope_set],
        Some(&scope_review),
    )
    .await;
    let slice_review =
        json!({"revision":3,"verdict":"ready","summary":"Bounded and ready","findings":[]});
    run(
        "INSERT INTO slice_candidate_reviews(tenant_id,workspace_id,candidate_set_id,\
         set_revision,payload) VALUES($1,$2,$3,3,$4)",
        &[rows.tenant, rows.workspace, rows.slice_set],
        Some(&slice_review),
    )
    .await;
    journal
}

/// The workspace, its setting, its journal, the two candidate sets and their drafts
/// and reviews, as text.
async fn held(pool: &PgPool, rows: &Rows) -> Vec<String> {
    let row = sqlx::query(
        "SELECT (SELECT pg_catalog.to_jsonb(w)::text FROM workspaces w WHERE w.id=$1), \
                (SELECT pg_catalog.to_jsonb(c)::text FROM advice_config c WHERE c.workspace_id=$1), \
                (SELECT pg_catalog.jsonb_agg(pg_catalog.to_jsonb(j) ORDER BY j.id)::text \
                   FROM advice_journal j WHERE j.workspace_id=$1), \
                (SELECT pg_catalog.jsonb_agg(pg_catalog.to_jsonb(s) ORDER BY s.id)::text \
                   FROM scope_candidate_sets s WHERE s.id=$2), \
                (SELECT pg_catalog.jsonb_agg(pg_catalog.to_jsonb(s) ORDER BY s.id)::text \
                   FROM slice_candidate_sets s WHERE s.id=$3), \
                (SELECT d.payload::text FROM scope_candidate_drafts d WHERE d.candidate_set_id=$2), \
                (SELECT d.payload::text FROM slice_candidate_drafts d WHERE d.candidate_set_id=$3), \
                (SELECT r.payload::text FROM scope_candidate_reviews r WHERE r.candidate_set_id=$2), \
                (SELECT r.payload::text FROM slice_candidate_reviews r WHERE r.candidate_set_id=$3)",
    )
    .bind(rows.workspace)
    .bind(rows.scope_set)
    .bind(rows.slice_set)
    .fetch_one(pool)
    .await
    .unwrap();
    (0..9).map(|column| row.get::<String, _>(column)).collect()
}

async fn upgrade(pool: &PgPool, runtime_role: &str) {
    // Every migration that came before the binding.
    migrate_before(pool, "0045", 44).await;
    let rows = seed(pool).await;
    let journal = seed_advice(pool, &rows).await;
    let before = held(pool, &rows).await;
    admin::migrate(pool, runtime_role).await.unwrap();
    let after = held(pool, &rows).await;
    assert_eq!(before, after);

    // The stored reviews read as reviews and would be written back unchanged.
    let payload: Value = serde_json::from_str(&after[7]).unwrap();
    let review: ScopeCandidateReview = serde_json::from_value(payload.clone()).unwrap();
    assert_eq!(json!(review), payload);
    let payload: Value = serde_json::from_str(&after[8]).unwrap();
    let review: SliceCandidateReview = serde_json::from_value(payload.clone()).unwrap();
    assert_eq!(json!(review), payload);

    // No backfill: advice given before the binding is kept nowhere and opens nothing.
    let count: i64 = sqlx::query_scalar(&format!("SELECT count(*) FROM {TABLE}"))
        .fetch_one(pool)
        .await
        .unwrap();
    assert_eq!(count, 0);

    // Row security is forced, as on the journal, and the runtime role may read and
    // insert rows and write the two departure columns and nothing else.
    let secured: (bool, bool) = sqlx::query_as(
        "SELECT relrowsecurity, relforcerowsecurity FROM pg_catalog.pg_class \
         WHERE oid=('public.' || $1)::regclass",
    )
    .bind(TABLE)
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(secured, (true, true));
    let may = async |privilege: &str| -> bool {
        sqlx::query_scalar("SELECT pg_catalog.has_table_privilege($1, $2, $3)")
            .bind(runtime_role)
            .bind(TABLE)
            .bind(privilege)
            .fetch_one(pool)
            .await
            .unwrap()
    };
    let may_update = async |column: &str| -> bool {
        sqlx::query_scalar("SELECT pg_catalog.has_column_privilege($1, $2, $3, 'UPDATE')")
            .bind(runtime_role)
            .bind(TABLE)
            .bind(column)
            .fetch_one(pool)
            .await
            .unwrap()
    };
    assert!(may("SELECT").await && may("INSERT").await);
    for refused in ["UPDATE", "DELETE", "TRUNCATE"] {
        assert!(!may(refused).await, "{refused}");
    }
    for column in ["departure_reason", "departed_at"] {
        assert!(may_update(column).await, "{column}");
    }
    for column in ["answer", "asks_change", "candidate_revision", "created_at"] {
        assert!(!may_update(column).await, "{column}");
    }

    // One answer per candidate, revision and point; a departure is a reason with its
    // time, and only on advice that asks for a change.
    let candidate = Uuid::new_v4();
    let keep = async |point: &str, level: &str, revision: i64, asks: bool| {
        sqlx::query(&format!(
            "INSERT INTO {TABLE}(tenant_id,workspace_id,journal_id,point,candidate_set_id,\
             level,candidate_id,candidate_revision,asks_change,answer) \
             VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,'{{\"advice\":\"split\"}}'::jsonb)"
        ))
        .bind(rows.tenant)
        .bind(rows.workspace)
        .bind(journal)
        .bind(point)
        .bind(rows.scope_set)
        .bind(level)
        .bind(candidate)
        .bind(revision)
        .bind(asks)
        .execute(pool)
        .await
        .is_ok()
    };
    assert!(keep("sizing", "program_to_scopes", 1, true).await);
    assert!(!keep("sizing", "program_to_scopes", 1, true).await, "twice");
    assert!(keep("sizing", "program_to_scopes", 2, false).await);
    assert!(keep("pipeline", "scope_to_slices", 1, true).await);
    assert!(!keep("intake", "program_to_scopes", 3, true).await, "point");
    assert!(!keep("sizing", "program", 3, true).await, "level");
    assert!(
        !keep("sizing", "program_to_scopes", 0, true).await,
        "revision"
    );
    let depart = async |revision: i64, reason: Option<&str>, timed: bool| {
        sqlx::query(&format!(
            "UPDATE {TABLE} SET departure_reason=$3, \
                 departed_at=CASE WHEN $4 THEN pg_catalog.clock_timestamp() END \
             WHERE candidate_id=$1 AND candidate_revision=$2 AND point='sizing'"
        ))
        .bind(candidate)
        .bind(revision)
        .bind(reason)
        .bind(timed)
        .execute(pool)
        .await
        .is_ok()
    };
    assert!(
        !depart(1, Some("It is one outcome"), false).await,
        "no time"
    );
    assert!(!depart(1, None, true).await, "no reason");
    assert!(!depart(1, Some("  "), true).await, "a blank reason");
    assert!(
        !depart(2, Some("It is one outcome"), true).await,
        "asks nothing"
    );
    assert!(depart(1, Some("It is one outcome"), true).await);
    let moved = sqlx::query(&format!(
        "UPDATE {TABLE} SET created_at=pg_catalog.clock_timestamp() WHERE candidate_id=$1"
    ))
    .bind(candidate)
    .execute(pool)
    .await;
    assert!(moved.is_err(), "created_at is immutable");
}

#[tokio::test]
async fn migration_0045_leaves_existing_workspaces_journal_rows_and_reviews_as_they_were() {
    in_own_database("tect_binding_upgrade", |pool, runtime_role| async move {
        upgrade(&pool, &runtime_role).await;
    })
    .await;
}
