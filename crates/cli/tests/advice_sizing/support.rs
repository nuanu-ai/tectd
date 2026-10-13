//! A real daemon with a loopback Jev behind it, and what the tests read back.
use crate::recovery_support::{Mcp, host_file, private_temp, tagged_url, tool_payload};
use serde_json::{Map, Value, json};
use sqlx::PgPool;
use std::{
    fs,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    os::unix::fs::{FileTypeExt, OpenOptionsExt, PermissionsExt},
    path::PathBuf,
    process::Stdio,
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};
use tect_postgres::admin;
use tokio::process::{Child, Command};
use uuid::Uuid;

pub const KEY: &str = "sizing-test-key-0123456789";
pub const BUDGET: usize = 8192;

/// How the loopback Jev answers the next requests.
#[derive(Clone, Copy)]
pub enum Jev {
    /// Each question gets the choice and probability this rule names for it.
    Answers(fn(&str) -> (&'static str, f64)),
    /// A well-formed answer after a pause longer than the daemon waits.
    Late(Duration),
    /// A success status with a body that is not an answer.
    Garbage,
    /// Answers by the rule, except that a request asking any question whose name
    /// ends with the suffix gets the body that is not an answer: one point of a
    /// command fails while another is answered.
    // This module is compiled once per advice binary; only the pipeline suite uses this.
    #[allow(dead_code)]
    AnswersBut(fn(&str) -> (&'static str, f64), &'static str),
    /// Each question gets the choice and probability the rule names, and one other
    /// criterion gets a probability of its own when the rule names one; the rest is
    /// shared by the remaining criteria. It tells a probability reported for one
    /// named criterion from the share every unchosen criterion would have.
    #[allow(dead_code)]
    Weighted(fn(&str) -> Weights),
}

/// What Jev chooses for a question with its probability, and another criterion with
/// the probability it is given.
pub type Weights = ((&'static str, f64), Option<(&'static str, f64)>);

pub struct FakeJev {
    pub endpoint: String,
    mode: Arc<Mutex<Jev>>,
    requests: Arc<Mutex<Vec<Value>>>,
}

impl FakeJev {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}/v1/systemone", listener.local_addr().unwrap());
        let mode = Arc::new(Mutex::new(Jev::Garbage));
        let requests = Arc::new(Mutex::new(Vec::new()));
        let (shared_mode, seen) = (mode.clone(), requests.clone());
        // The thread ends with the test process; it holds nothing but the socket.
        thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let (mode, seen) = (shared_mode.clone(), seen.clone());
                thread::spawn(move || serve(stream, &mode, &seen));
            }
        });
        Self {
            endpoint,
            mode,
            requests,
        }
    }

    pub fn answer(&self, mode: Jev) {
        *self.mode.lock().unwrap() = mode;
    }

    /// Every request body that reached Jev, in arrival order.
    pub fn requests(&self) -> Vec<Value> {
        self.requests.lock().unwrap().clone()
    }

    pub fn calls(&self) -> usize {
        self.requests.lock().unwrap().len()
    }

    /// The requests of one advice point, in arrival order, told by the questions
    /// only that point asks: `.size` for `sizing`, `.pipeline` for `pipeline`.
    // This module is compiled once per advice binary; not every suite reads by point.
    #[allow(dead_code)]
    pub fn asked(&self, suffix: &str) -> Vec<Value> {
        let of_point = |request: &Value| {
            let questions = request["questions"].as_object().unwrap();
            questions.keys().any(|name| name.ends_with(suffix))
        };
        self.requests().into_iter().filter(of_point).collect()
    }
}

fn serve(mut stream: TcpStream, mode: &Mutex<Jev>, seen: &Mutex<Vec<Value>>) {
    let mut raw = Vec::new();
    let mut chunk = [0; 8192];
    let body = loop {
        let read = stream.read(&mut chunk).unwrap();
        assert!(read > 0, "Jev request ended early");
        raw.extend_from_slice(&chunk[..read]);
        let Some(end) = raw.windows(4).position(|window| window == b"\r\n\r\n") else {
            continue;
        };
        let head = String::from_utf8_lossy(&raw[..end]).to_ascii_lowercase();
        assert!(head.contains(&format!("authorization: bearer {KEY}")));
        let length: usize = head
            .lines()
            .find_map(|line| line.strip_prefix("content-length: "))
            .unwrap()
            .parse()
            .unwrap();
        if raw.len() >= end + 4 + length {
            break raw[end + 4..end + 4 + length].to_vec();
        }
    };
    let request: Value = serde_json::from_slice(&body).unwrap();
    seen.lock().unwrap().push(request.clone());
    let mode = *mode.lock().unwrap();
    let answer = match mode {
        Jev::Garbage => "{\"answers\":\"none\"}".to_owned(),
        Jev::Late(pause) => {
            thread::sleep(pause);
            answers(&request, |_| ("ABSTAIN", 0.9))
        }
        Jev::Answers(rule) => answers(&request, rule),
        Jev::Weighted(rule) => weighted(&request, rule),
        Jev::AnswersBut(rule, failing) => {
            let questions = request["questions"].as_object().unwrap();
            if questions.keys().any(|name| name.ends_with(failing)) {
                "{\"answers\":\"none\"}".to_owned()
            } else {
                answers(&request, rule)
            }
        }
    };
    let head = format!(
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        answer.len()
    );
    // The daemon may have stopped waiting; a failed write is the late case.
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(answer.as_bytes());
}

/// An answer the acceptance rules take: the chosen criterion at its probability and
/// the remainder shared by the other criteria of the question.
fn answers(request: &Value, rule: impl Fn(&str) -> (&'static str, f64)) -> String {
    let mut given = Map::new();
    for (name, question) in request["questions"].as_object().unwrap() {
        let criteria = question["criteria"].as_object().unwrap();
        let (choice, probability) = rule(name);
        assert!(
            criteria.contains_key(choice),
            "{name} does not offer {choice}"
        );
        let rest = (1.0 - probability) / (criteria.len() - 1) as f64;
        let probabilities: Map<String, Value> = criteria
            .keys()
            .map(|criterion| {
                let share = if criterion == choice {
                    probability
                } else {
                    rest
                };
                (criterion.clone(), json!(share))
            })
            .collect();
        given.insert(
            name.clone(),
            json!({"type":"choice","choice":choice,"probabilities":probabilities,
                "confidence":0.4}),
        );
    }
    json!({"model":"jev-2026-10","answers":given,
        "usage":{"input_tokens":640,"output_tokens":90}})
    .to_string()
}

/// An answer the acceptance rules take, as `answers` gives it, with one more named
/// criterion at a probability of its own.
fn weighted(request: &Value, rule: impl Fn(&str) -> Weights) -> String {
    let mut given = Map::new();
    for (name, question) in request["questions"].as_object().unwrap() {
        let criteria = question["criteria"].as_object().unwrap();
        let ((choice, probability), other) = rule(name);
        assert!(
            criteria.contains_key(choice),
            "{name} does not offer {choice}"
        );
        let named = other.filter(|(criterion, _)| *criterion != choice);
        if let Some((criterion, _)) = named {
            assert!(
                criteria.contains_key(criterion),
                "{name} does not offer {criterion}"
            );
        }
        let taken = probability + named.map_or(0.0, |(_, share)| share);
        let unnamed = criteria.len() - 1 - usize::from(named.is_some());
        let rest = (1.0 - taken) / unnamed as f64;
        let probabilities: Map<String, Value> = criteria
            .keys()
            .map(|criterion| {
                let share = match named {
                    _ if criterion == choice => probability,
                    Some((other, share)) if criterion == other => share,
                    _ => rest,
                };
                (criterion.clone(), json!(share))
            })
            .collect();
        given.insert(
            name.clone(),
            json!({"type":"choice","choice":choice,"probabilities":probabilities,
                "confidence":0.4}),
        );
    }
    json!({"model":"jev-2026-10","answers":given,
        "usage":{"input_tokens":640,"output_tokens":90}})
    .to_string()
}

/// One daemon with the Jev provider configured, one enrolled host, and the operator's
/// connection for looking at the tables.
pub struct Stand {
    pub pool: PgPool,
    pub tenant: Uuid,
    pub jev: FakeJev,
    pub root: PathBuf,
    socket: PathBuf,
    config: PathBuf,
    workspaces: Mutex<usize>,
    _daemon: Child,
    _temp: tempfile::TempDir,
}

impl Stand {
    /// `timeout_ms` is how long the daemon waits for Jev.
    pub async fn start(timeout_ms: &str) -> Self {
        Self::start_at(None, KEY, timeout_ms).await
    }

    /// The daemon asks `endpoint` with `key`; without an endpoint, the loopback Jev.
    pub async fn start_at(endpoint: Option<&str>, key: &str, timeout_ms: &str) -> Self {
        Self::launch(Some((endpoint, key, timeout_ms))).await
    }

    /// A daemon started with no Jev setting at all: it has no provider.
    // This module is compiled once per advice binary; only the intake suite uses this.
    #[allow(dead_code)]
    pub async fn start_without_provider() -> Self {
        Self::launch(None).await
    }

    async fn launch(provider: Option<(Option<&str>, &str, &str)>) -> Self {
        let admin_url = std::env::var("TECT_TEST_ADMIN_URL").expect("TECT_TEST_ADMIN_URL required");
        let runtime =
            std::env::var("TECT_TEST_RUNTIME_URL").expect("TECT_TEST_RUNTIME_URL required");
        let role =
            std::env::var("TECT_TEST_RUNTIME_ROLE").expect("TECT_TEST_RUNTIME_ROLE required");
        let pool = PgPool::connect(&admin_url).await.unwrap();
        admin::migrate(&pool, &role).await.unwrap();
        let temp = private_temp();
        let root = temp.path().canonicalize().unwrap();
        let jev = FakeJev::start();
        let socket = root.join("sizing.sock");
        let url = tagged_url(&runtime, &format!("tect-advice-sizing-{}", Uuid::new_v4()));
        let log = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .mode(0o600)
            .open(socket.with_extension("stderr"))
            .unwrap();
        let mut command = Command::new(env!("CARGO_BIN_EXE_tectd"));
        command
            .env("TECT_DATABASE_URL", &url)
            .env("TECT_SOCKET", &socket)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::from(log))
            .kill_on_drop(true);
        // Exactly this Jev environment, whatever the test runner has.
        for name in tect_host::JEV_ENVIRONMENT {
            command.env_remove(name);
        }
        if let Some((endpoint, key, timeout_ms)) = provider {
            command
                .env("TECT_JEV_ENDPOINT", endpoint.unwrap_or(&jev.endpoint))
                .env("TECT_JEV_MODEL", "jev-latest")
                .env("TECT_JEV_TIMEOUT_MS", timeout_ms)
                .env("TYPESAFE_API_KEY", key);
        }
        let mut daemon = command.spawn().unwrap();
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                assert!(daemon.try_wait().unwrap().is_none(), "daemon exited");
                if fs::symlink_metadata(&socket).is_ok_and(|meta| {
                    meta.file_type().is_socket() && meta.permissions().mode() & 0o777 == 0o600
                }) {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        let enrollment = admin::enroll_host(&pool, None, vec![root.to_string_lossy().into_owned()])
            .await
            .unwrap();
        let config = root.join("host.json");
        host_file(&config, &enrollment.auth);
        Self {
            pool,
            tenant: enrollment.tenant_id,
            jev,
            root,
            socket,
            config,
            workspaces: Mutex::new(0),
            _daemon: daemon,
            _temp: temp,
        }
    }

    /// A new native session in a workspace of its own, with a repository to register.
    pub async fn workspace(&self) -> (Mcp, PathBuf) {
        let number = {
            let mut count = self.workspaces.lock().unwrap();
            *count += 1;
            *count
        };
        let repo = self.root.join(format!("source-{number}"));
        crate::planning::repository(&repo);
        let key = format!("advice-sizing-{}", Uuid::new_v4().simple());
        let client = Mcp::start(
            &self.socket,
            &self.config,
            &Uuid::new_v4().to_string(),
            &key,
        )
        .await;
        (client, repo)
    }

    /// Set the product level of a Program as the operator, or clear it: a Program
    /// opened before the field existed has none.
    pub async fn set_mode(&self, program_id: &Value, mode: Option<&str>) {
        let id = Uuid::parse_str(program_id.as_str().unwrap()).unwrap();
        let updated = sqlx::query("UPDATE programs SET mode=$1 WHERE tenant_id=$2 AND id=$3")
            .bind(mode)
            .bind(self.tenant)
            .bind(id)
            .execute(&self.pool)
            .await
            .unwrap();
        assert_eq!(updated.rows_affected(), 1);
    }

    pub async fn count(&self, table: &str) -> i64 {
        sqlx::query_scalar(&format!("SELECT count(*) FROM {table} WHERE tenant_id=$1"))
            .bind(self.tenant)
            .fetch_one(&self.pool)
            .await
            .unwrap()
    }

    /// The newest journal row as the operator sees it, with the request body as JSON.
    pub async fn newest_row(&self) -> Value {
        sqlx::query_scalar(
            "SELECT pg_catalog.to_jsonb(j) - 'request_body' - 'response_body' \
                 || pg_catalog.jsonb_build_object('request', \
                     pg_catalog.convert_from(request_body, 'UTF8')::jsonb) \
             FROM advice_journal j WHERE tenant_id=$1 \
             ORDER BY created_at DESC, id DESC LIMIT 1",
        )
        .bind(self.tenant)
        .fetch_one(&self.pool)
        .await
        .unwrap()
    }

    /// The newest journal row of one advice point, read the same way. A Slice draft
    /// save writes a row per point, so "newest" alone does not name one.
    #[allow(dead_code)]
    pub async fn newest_row_of(&self, point: &str) -> Value {
        sqlx::query_scalar(
            "SELECT pg_catalog.to_jsonb(j) - 'request_body' - 'response_body' \
                 || pg_catalog.jsonb_build_object('request', \
                     pg_catalog.convert_from(request_body, 'UTF8')::jsonb) \
             FROM advice_journal j WHERE tenant_id=$1 AND point=$2 \
             ORDER BY created_at DESC, id DESC LIMIT 1",
        )
        .bind(self.tenant)
        .bind(point)
        .fetch_one(&self.pool)
        .await
        .unwrap()
    }

    /// Journal rows of one advice point: all of them, and those whose request went out.
    #[allow(dead_code)]
    pub async fn rows_of(&self, point: &str) -> (i64, i64) {
        sqlx::query_as(
            "SELECT count(*), count(*) FILTER (WHERE called) FROM advice_journal \
             WHERE tenant_id=$1 AND point=$2",
        )
        .bind(self.tenant)
        .bind(point)
        .fetch_one(&self.pool)
        .await
        .unwrap()
    }
}

/// Enable or disable Jev for the client's workspace.
pub async fn configure(client: &mut Mcp, enabled: bool, expected_revision: i64) {
    let mut params = json!({"enabled":enabled,"expected_revision":expected_revision});
    if enabled {
        params["daily_call_limit"] = json!(20);
    }
    let status = command(client, "workspace.advisory.configure", params).await;
    assert_eq!(status.0["enabled"], enabled);
    assert_eq!(status.0["revision"], expected_revision + 1);
}

/// One command through the bridge: its payload and the envelope bytes of its reply.
pub async fn command(client: &mut Mcp, route: &str, params: Value) -> (Value, usize) {
    let response = client
        .exchange(
            "tools/call",
            json!({"name":"command","arguments":{"route":route,"params":params}}),
        )
        .await;
    assert!(
        response.get("error").is_none() && response["result"]["isError"] != true,
        "{response}"
    );
    assert!(!response.to_string().contains(KEY));
    let envelope = serde_json::to_vec(&response["result"]).unwrap().len();
    (tool_payload(&response), envelope)
}

/// The `sizing` rows of the journal through the audit route, newest first, without bodies.
pub async fn audit(client: &mut Mcp) -> Value {
    let response = client
        .exchange(
            "tools/call",
            json!({"name":"query","arguments":{"route":"workspace.advisory.audit",
                "params":{"limit":10,"point":"sizing","bodies":false}}}),
        )
        .await;
    tool_payload(&response)
}
