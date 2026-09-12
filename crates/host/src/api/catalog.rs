use crate::tools::object_schema;
use serde_json::{Value, json};
use tect_domain::{MAX_SOURCE_PATH_BYTES, MAX_WORKTREES};

#[derive(Clone)]
pub(crate) struct RouteSpec {
    pub tool: &'static str,
    pub route: &'static str,
    pub internal: &'static str,
    pub summary: &'static str,
    pub conditions: &'static str,
    pub effects: &'static str,
    pub retry: &'static str,
    pub schema: Value,
    pub example: Value,
}

impl RouteSpec {
    pub(crate) fn aliases(&self) -> &'static [&'static str] {
        match self.route {
            "program.get" => &["read program", "получить программу", "прочитать программу"],
            "program.list" => &["list programs", "список программ"],
            "source.list" => &["list sources", "список исходников"],
            "setup.get" => &["read setup", "получить настройку"],
            "workspace.open" => &["open workspace", "открыть рабочее пространство"],
            "source.register" => &["register source", "зарегистрировать исходник"],
            "session.select_worktrees" => &["select worktrees", "выбрать worktree"],
            "program.begin" => &[
                "begin program",
                "create program",
                "start program",
                "начать программу",
                "создать программу",
                "открыть программу",
            ],
            "program.save" => &["save program", "сохранить программу"],
            "program.record_input" => &["program reply", "ответ для программы"],
            "setup.inspect" => &["inspect agents", "проверить agents"],
            "setup.begin" => &["begin setup", "начать настройку"],
            "setup.save" => &["save setup", "сохранить настройку"],
            "setup.record_input" => &["setup reply", "ответ для настройки"],
            "setup.apply" => &["apply setup", "создать agents", "применить настройку"],
            _ => &[],
        }
    }
}

fn uuid() -> Value {
    json!({"type":"string","format":"uuid"})
}

fn text() -> Value {
    json!({"type":"string","minLength":1})
}

fn nullable_text() -> Value {
    json!({"type":["string","null"]})
}

fn page_limit() -> Value {
    json!({"type":"integer","minimum":1,"maximum":100,"default":25})
}

macro_rules! route {
    ($tool:expr, $name:expr, $internal:expr, $summary:expr, $conditions:expr,
     $effects:expr, $retry:expr, $schema:expr, $example:expr $(,)?) => {
        RouteSpec {
            tool: $tool,
            route: $name,
            internal: $internal,
            summary: $summary,
            conditions: $conditions,
            effects: $effects,
            retry: $retry,
            schema: $schema,
            example: $example,
        }
    };
}

pub(crate) fn routes() -> Vec<RouteSpec> {
    let example_id = "00000000-0000-4000-8000-000000000001";
    vec![
        route!(
            "query",
            "program.get",
            "get_program",
            "Read one Program and a bounded page of exact original inputs.",
            "Requires an authenticated open native session and an accessible Program. Omitted after_input starts at the saved input cursor; explicit null is invalid.",
            "Reads a consistent database snapshot only.",
            "Safe to repeat. Follow next_after_input exactly when present.",
            object_schema(
                json!({"program_id":uuid(),"after_input":{"type":"integer","minimum":0},"limit":page_limit()}),
                json!(["program_id"]),
            ),
            json!({"program_id":example_id,"after_input":0,"limit":25}),
        ),
        route!(
            "query",
            "program.list",
            "list_programs",
            "List a bounded Program page with unfinished work first.",
            "Requires an authenticated open native session. Omitted after starts at the first page; explicit null is invalid.",
            "Reads a consistent database snapshot only.",
            "Safe to repeat. Use the returned cursor unchanged.",
            object_schema(
                json!({"after":{"type":"string","pattern":"^[wr]:[0-9a-fA-F-]+$"},"limit":page_limit()}),
                json!([]),
            ),
            json!({"limit":25}),
        ),
        route!(
            "query",
            "source.list",
            "list_sources",
            "List a bounded page of registered worktrees.",
            "Requires an authenticated open native session. Omitted after starts at the first page; explicit null is invalid.",
            "Reads the workspace and host scoped source catalog only.",
            "Safe to repeat. Use next_after unchanged when present.",
            object_schema(
                json!({"after":uuid(),"limit":{"type":"integer","minimum":1,"maximum":MAX_WORKTREES}}),
                json!(["limit"]),
            ),
            json!({"limit":25}),
        ),
        route!(
            "query",
            "setup.get",
            "get_setup",
            "Read a setup draft, exact input page, and current file observation.",
            "Requires an authenticated open native session, its bound task directory, current setup grant, and an accessible setup. Omitted after_input uses saved coverage; explicit null is invalid.",
            "Reads the database and explicitly revalidates the bound AGENTS.md target without writing it.",
            "Safe to repeat. Recovery should start with after_input 0.",
            object_schema(
                json!({"setup_id":uuid(),"after_input":{"type":"integer","minimum":0},"limit":page_limit()}),
                json!(["setup_id"]),
            ),
            json!({"setup_id":example_id,"after_input":0,"limit":25}),
        ),
        route!(
            "command",
            "workspace.open",
            "open_workspace",
            "Create or recover this native session's logical workspace binding.",
            "Requires valid native identity, host credential, tenant, and configured workspace key.",
            "Atomically creates missing workspace, membership, native session, and creation events, or reads the existing binding.",
            "Idempotent for the same authenticated host, native session, and workspace key.",
            object_schema(json!({}), json!([])),
            json!({}),
        ),
        route!(
            "command",
            "source.register",
            "register_source",
            "Register one existing Git worktree in the logical workspace.",
            "Requires an authenticated open session and a canonical Git worktree/common directory under current source grants.",
            "Reads Git identity and atomically records repository/worktree rows; it never changes Git.",
            "Idempotent for the same physical worktree.",
            object_schema(
                json!({"path":{"type":"string","minLength":1,"maxLength":MAX_SOURCE_PATH_BYTES}}),
                json!(["path"]),
            ),
            json!({"path":"/absolute/source/worktree"}),
        ),
        route!(
            "command",
            "session.select_worktrees",
            "select_worktrees",
            "Replace this native session's complete selected worktree set.",
            "Requires an authenticated open session and 0 to 100 unique accessible worktree IDs.",
            "Atomically replaces only this session's selection; an invalid member preserves the old set.",
            "Safe to repeat with the same complete set.",
            object_schema(
                json!({"worktree_ids":{"type":"array","items":uuid(),"maxItems":MAX_WORKTREES,"uniqueItems":true}}),
                json!(["worktree_ids"]),
            ),
            json!({"worktree_ids":[]}),
        ),
        route!(
            "command",
            "program.begin",
            "begin_program",
            "Persist one exact original narrative and create a resumable Program draft.",
            "Requires an authenticated open session, a non-nil request_id, and nonblank original input.",
            "Creates one database Program and first immutable input in one transaction.",
            "Repeat only with the same request_id and byte-identical input; changed input conflicts.",
            object_schema(
                json!({"request_id":uuid(),"input":text()}),
                json!(["request_id", "input"]),
            ),
            json!({"request_id":example_id,"input":"Complete original user narrative"}),
        ),
        route!(
            "command",
            "program.save",
            "save_program",
            "Save a revision-checked Program patch and optionally open the same Program.",
            "Requires current revision and input_cursor. Omission preserves nullable text fields; null clears them. complete true requires six coherent fields, no question, and full input coverage.",
            "Atomically updates the Program revision; it does not create Scope or execute work.",
            "On stale or uncertain result, reload program.get and reconcile before retrying.",
            object_schema(
                json!({"program_id":uuid(),"revision":{"type":"integer","minimum":1},"input_cursor":{"type":"integer","minimum":0},"name":nullable_text(),"intent":nullable_text(),"basis":nullable_text(),"boundaries":nullable_text(),"constraints":nullable_text(),"success":nullable_text(),"working_notes":nullable_text(),"pending_question":nullable_text(),"complete":{"type":"boolean","default":false}}),
                json!(["program_id", "revision", "input_cursor"]),
            ),
            json!({"program_id":example_id,"revision":1,"input_cursor":1,"name":"Example Program","complete":false}),
        ),
        route!(
            "command",
            "program.record_input",
            "record_program_input",
            "Append one complete original reply or correction to a Program.",
            "Requires an authenticated open session, accessible Program, non-nil request_id, and nonblank exact input.",
            "Atomically appends immutable input and advances the Program revision.",
            "Repeat only with the same request_id and byte-identical input.",
            object_schema(
                json!({"program_id":uuid(),"request_id":uuid(),"input":text()}),
                json!(["program_id", "request_id", "input"]),
            ),
            json!({"program_id":example_id,"request_id":"00000000-0000-4000-8000-000000000002","input":"Complete original reply"}),
        ),
        route!(
            "command",
            "setup.inspect",
            "inspect_setup",
            "Inspect the fixed AGENTS.md in the current task launch directory.",
            "Requires host authentication and an open session. Omission reports context_unknown. A supplied path must be the known task launch directory and remain within current setup grants.",
            "May bind the verified physical directory in the database and read filesystem state; it never publishes a file.",
            "Safe to repeat. Unknown or unavailable never proves the file absent.",
            object_schema(
                json!({"task_directory":{"type":"string","minLength":1,"maxLength":MAX_SOURCE_PATH_BYTES}}),
                json!([]),
            ),
            json!({"task_directory":"/absolute/task/launch-directory"}),
        ),
        route!(
            "command",
            "setup.begin",
            "begin_setup",
            "Persist one exact narrative and create a setup draft after verified absence.",
            "Requires an authenticated open session, bound granted directory with freshly verified missing AGENTS.md, non-nil request_id, and nonblank exact input.",
            "Creates one database setup and first immutable input; it does not publish a file.",
            "Repeat only with the same request_id and byte-identical input.",
            object_schema(
                json!({"request_id":uuid(),"input":text()}),
                json!(["request_id", "input"]),
            ),
            json!({"request_id":example_id,"input":"Complete original company and work narrative"}),
        ),
        route!(
            "command",
            "setup.save",
            "save_setup",
            "Save a revision-checked setup draft or durable ready intent.",
            "Requires current revision, input_cursor, and ready. Omission preserves nullable patches; null clears. ready true requires coherent content, no pending question, and full input coverage.",
            "Atomically updates only setup state; it does not publish AGENTS.md.",
            "On stale or uncertain result, reload setup.get from input 0 before retrying.",
            object_schema(
                json!({"setup_id":uuid(),"revision":{"type":"integer","minimum":1},"input_cursor":{"type":"integer","minimum":0},"ready":{"type":"boolean"},"content":nullable_text(),"working_notes":nullable_text(),"pending_question":nullable_text()}),
                json!(["setup_id", "revision", "input_cursor", "ready"]),
            ),
            json!({"setup_id":example_id,"revision":1,"input_cursor":1,"ready":false,"content":"# Workspace instructions"}),
        ),
        route!(
            "command",
            "setup.record_input",
            "record_setup_input",
            "Append one complete original reply or correction to a setup.",
            "Requires current setup revision, non-nil request_id, nonblank exact input, current binding, and setup grant.",
            "Atomically appends immutable input and advances the setup revision; it does not publish a file.",
            "Repeat only with the same revision, request_id, and byte-identical input.",
            object_schema(
                json!({"setup_id":uuid(),"revision":{"type":"integer","minimum":1},"request_id":uuid(),"input":text()}),
                json!(["setup_id", "revision", "request_id", "input"]),
            ),
            json!({"setup_id":example_id,"revision":1,"request_id":"00000000-0000-4000-8000-000000000002","input":"Complete original reply"}),
        ),
        route!(
            "execute",
            "setup.apply",
            "apply_setup",
            "Publish the durable ready setup as the fixed AGENTS.md and verify exact bytes.",
            "Requires current authenticated binding/grant and the exact ready setup revision. The complete saved content must be shown before this call.",
            "Exclusively creates AGENTS.md or adopts exact matching bytes, verifies readback, then records applied status. It never overwrites a different file.",
            "After an uncertain result, repeat the same setup_id and ready revision. A changed or removed applied file is preserved as a conflict.",
            object_schema(
                json!({"setup_id":uuid(),"revision":{"type":"integer","minimum":1}}),
                json!(["setup_id", "revision"]),
            ),
            json!({"setup_id":example_id,"revision":2}),
        ),
    ]
}
