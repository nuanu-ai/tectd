use serde_json::{Value, json};
use sqlx::{PgPool, postgres::PgRow};
use std::{
    collections::{BTreeMap, VecDeque},
    future::Future,
    pin::Pin,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};
use tect_application::{AdviceProvider, AdviceRequest, WorkspaceService};
use tect_domain::*;
use tect_postgres::{PgStore, admin};
use uuid::Uuid;

pub const ROUTE: &str = "scope.candidates.save";
pub const REQUEST_BODY: &[u8] = b"{\"model\":\"jev-latest\"}";
pub const RESPONSE_BODY: &[u8] = b"{\"answers\":{\"size\":{}}}";

type Scripted = std::result::Result<AdviceReply, AdviceFailure>;
/// called, send_certainty, outcome is empty, request body is empty
type InFlight = (bool, String, bool, bool);

/// A provider that answers from a script and looks at the journal, as the operator,
/// at the moment it is asked.
pub struct ScriptedProvider {
    configured: bool,
    script: Mutex<VecDeque<Scripted>>,
    calls: AtomicUsize,
    operator: PgPool,
    tenant: Uuid,
    in_flight: Mutex<Vec<InFlight>>,
}

impl ScriptedProvider {
    pub fn push_reply(&self, reply: AdviceReply) {
        self.script.lock().unwrap().push_back(Ok(reply));
    }

    pub fn push_failure(&self, failure: AdviceFailure) {
        self.script.lock().unwrap().push_back(Err(failure));
    }

    pub fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }

    pub fn seen_in_flight(&self) -> Vec<InFlight> {
        self.in_flight.lock().unwrap().clone()
    }
}

impl AdviceProvider for ScriptedProvider {
    fn configured(&self) -> bool {
        self.configured
    }

    fn ask<'life0, 'life1, 'async_trait>(
        &'life0 self,
        _: &'life1 AdviceQuestionSet,
    ) -> Pin<Box<dyn Future<Output = Scripted> + Send + 'async_trait>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait,
    {
        Box::pin(async move {
            self.calls.fetch_add(1, Ordering::SeqCst);
            // Another connection sees the row: it was committed before the call.
            let row: InFlight = sqlx::query_as(
                "SELECT called, send_certainty, outcome IS NULL, request_body IS NULL \
                 FROM advice_journal WHERE tenant_id=$1 ORDER BY created_at DESC, id DESC LIMIT 1",
            )
            .bind(self.tenant)
            .fetch_one(&self.operator)
            .await
            .unwrap();
            self.in_flight.lock().unwrap().push(row);
            self.script
                .lock()
                .unwrap()
                .pop_front()
                .expect("a scripted provider result for every call")
        })
    }
}

pub struct Fixture {
    /// Operator connection: owner of the tables, used to seed and to observe.
    pub pool: PgPool,
    pub tenant: Uuid,
    pub context: RequestContext,
    pub session: Uuid,
    pub service: Arc<WorkspaceService>,
    pub provider: Arc<ScriptedProvider>,
}

pub async fn fixture(provider_configured: bool) -> Fixture {
    let admin_url = std::env::var("TECT_TEST_ADMIN_URL").expect("TECT_TEST_ADMIN_URL required");
    let runtime_url =
        std::env::var("TECT_TEST_RUNTIME_URL").expect("TECT_TEST_RUNTIME_URL required");
    let role = std::env::var("TECT_TEST_RUNTIME_ROLE").expect("TECT_TEST_RUNTIME_ROLE required");
    let pool = PgPool::connect(&admin_url).await.unwrap();
    admin::migrate(&pool, &role).await.unwrap();
    let enrollment = admin::enroll_host(&pool, None, Vec::new()).await.unwrap();
    let provider = Arc::new(ScriptedProvider {
        configured: provider_configured,
        script: Mutex::new(VecDeque::new()),
        calls: AtomicUsize::new(0),
        operator: pool.clone(),
        tenant: enrollment.tenant_id,
        in_flight: Mutex::new(Vec::new()),
    });
    let store = Arc::new(PgStore::connect(&runtime_url, 6).await.unwrap());
    let service = Arc::new(
        WorkspaceService::new(
            store,
            Arc::new(tect_host::GitSourceInspector),
            Arc::new(tect_host::LocalSetupFiles),
        )
        .with_advice_provider(provider.clone()),
    );
    let context = RequestContext {
        auth: enrollment.auth.clone(),
        native_session_id: Uuid::new_v4().to_string(),
        workspace_key: format!("advice-{}", Uuid::new_v4().simple()),
    };
    let state = service.open_workspace(&context).await.unwrap();
    Fixture {
        pool,
        tenant: enrollment.tenant_id,
        session: state.session.unwrap().id,
        context,
        service,
        provider,
    }
}

impl Fixture {
    /// Another native session of the same host and workspace key, not yet opened.
    pub fn peer_context(&self) -> RequestContext {
        RequestContext {
            native_session_id: Uuid::new_v4().to_string(),
            ..self.context.clone()
        }
    }

    pub async fn configure(
        &self,
        enabled: bool,
        daily_call_limit: Option<i64>,
        expected_revision: i64,
    ) -> AdviceConfigStatus {
        self.service
            .configure_advice(
                &self.context,
                &ConfigureAdvice {
                    enabled,
                    daily_call_limit,
                    expected_revision,
                },
            )
            .await
            .unwrap()
    }

    pub async fn advise(&self, point: &str, work: &AdviceWorkRefs) -> Option<AdviceField> {
        let questions = questions();
        let request = AdviceRequest {
            point,
            route: ROUTE,
            work,
            questions: &questions,
        };
        self.service.advise(&self.context, &request).await.unwrap()
    }

    pub async fn count(&self, table: &str) -> i64 {
        sqlx::query_scalar(&format!("SELECT count(*) FROM {table} WHERE tenant_id=$1"))
            .bind(self.tenant)
            .fetch_one(&self.pool)
            .await
            .unwrap()
    }

    pub async fn newest_row(&self) -> PgRow {
        sqlx::query(
            "SELECT * FROM advice_journal WHERE tenant_id=$1 ORDER BY created_at DESC, id DESC LIMIT 1",
        )
        .bind(self.tenant)
        .fetch_one(&self.pool)
        .await
        .unwrap()
    }

    /// Every row version of this tenant in the tables a disabled Jev could touch.
    pub async fn versions(&self) -> Vec<Vec<String>> {
        let mut result = Vec::new();
        for table in [
            "advice_config",
            "advice_journal",
            "workspaces",
            "memberships",
            "agent_sessions",
            "workspace_events",
        ] {
            result.push(
                sqlx::query_scalar::<_, String>(&format!(
                    "SELECT xmin::text || ':' || row_to_json(t)::text FROM {table} t \
                     WHERE tenant_id=$1 ORDER BY row_to_json(t)::text"
                ))
                .bind(self.tenant)
                .fetch_all(&self.pool)
                .await
                .unwrap(),
            );
        }
        result
    }
}

pub fn questions() -> AdviceQuestionSet {
    serde_json::from_value(json!({
        "state": {"task": "Add invitations"},
        "questions": {"size": {"instructions": "Choose the size class.",
            "criteria": {"small": "One sitting.", "large": "Several days.",
                "ABSTAIN": "Not enough to tell."}}}
    }))
    .unwrap()
}

fn answers(choice: &str, small: f64, large: f64, abstain: f64) -> BTreeMap<String, AdviceAnswer> {
    let probabilities = [
        ("small", small),
        ("large", large),
        (ADVICE_ABSTAIN, abstain),
    ]
    .into_iter()
    .map(|(name, value)| (name.to_owned(), value))
    .collect();
    [(
        "size".to_owned(),
        AdviceAnswer {
            choice: choice.into(),
            probabilities,
            confidence: Some(0.8),
        },
    )]
    .into_iter()
    .collect()
}

/// An accepted answer whose provider reported input tokens only.
pub fn reply(choice: &str, small: f64, large: f64, abstain: f64) -> AdviceReply {
    AdviceReply {
        answers: answers(choice, small, large, abstain),
        exchange: AdviceExchange {
            model: Some("jev-latest".into()),
            input_tokens: Some(120),
            output_tokens: None,
            latency_ms: Some(42),
            request_body: Some(REQUEST_BODY.to_vec()),
            response_body: Some(RESPONSE_BODY.to_vec()),
        },
    }
}

/// The reply field an accepted answer becomes.
pub fn advised(choice: &str, small: f64, large: f64, abstain: f64) -> AdviceField {
    serde_json::from_value(json!({"status":"advised",
        "answers": answers(choice, small, large, abstain)}))
    .unwrap()
}

pub fn failed(
    reason: AdviceUnavailableReason,
    send_certainty: AdviceSendCertainty,
    detail: &str,
    request_sent: bool,
) -> AdviceFailure {
    AdviceFailure {
        reason,
        send_certainty,
        detail: detail.into(),
        exchange: AdviceExchange {
            model: Some("jev-latest".into()),
            request_body: request_sent.then(|| REQUEST_BODY.to_vec()),
            ..AdviceExchange::default()
        },
    }
}

pub fn audit(query: Value) -> AdviceAuditQuery {
    let query: AdviceAuditQuery = serde_json::from_value(query).unwrap();
    query.validate().unwrap();
    query
}
