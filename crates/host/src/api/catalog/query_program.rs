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
            "task.source.get",
            "get_matrix_task",
            "Read the current immutable Engineering Matrix source revision for one task.",
            "Requires an authenticated native session bound to the task's workspace.",
            "Reads the current task source revision and its recorded identity and digest.",
            "Safe to repeat; a missing task returns not_found.",
            object_schema(json!({"task_id":uuid()}), json!(["task_id"])),
            json!({"task_id":example_id}),
        ),
        route!(
            "query",
            "engineering.matrix.disposition.get",
            "get_matrix_disposition",
            "Read one saved explicit Matrix disposition by task and request ID.",
            "Requires an authenticated native session bound to the task workspace; both IDs must be non-nil.",
            "Reads the immutable typed decision, actor/session provenance, and binding digests without raw Jev content.",
            "Safe to repeat; an absent request returns not_found.",
            object_schema(
                json!({"task_id":uuid(),"request_id":uuid()}),
                json!(["task_id", "request_id"])
            ),
            json!({"task_id":example_id,"request_id":"00000000-0000-4000-8000-000000000002"}),
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
            "Requires valid native identity, host credential, tenant, and configured workspace key. A verifier requires an existing workspace and explicit membership.",
            "Owners may create missing workspace, membership, session, and creation events. Verifiers create only their native session and receive no owner workflow actions.",
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
            "task.source.record",
            "record_matrix_task",
            "Record one exact Engineering Matrix factual input revision and optional owner-authored engineering alternatives for a task.",
            "Requires an authenticated open native session, non-nil task and request IDs, revision 1 or the immediate successor of the expected current revision, valid tagged factual input and an optional choice set bound to the exact task/revision. Combined input and choice-set JSON is capped at 1 MiB; at most 1024 reported facts and five candidates. Zero or one candidate is recorded but not eligible for ranking. Choice-set assumptions must reference Matrix fact IDs in this input.",
            "Atomically stores the immutable revision, input digest, and optional choice-set digest in this workspace; source authority remains bound to the native session.",
            "Repeat the same request_id with identical revision, input, and choice set. On uncertainty, read task.source.get before another write.",
            object_schema(
                json!({"task_id":uuid(),"revision":{"type":"integer","minimum":1},"expected_current_revision":{"type":"integer","minimum":0},"request_id":uuid(),"input":matrix_task_schema::input(),"choice_set":matrix_task_schema::choice_set()}),
                json!([
                    "task_id",
                    "revision",
                    "expected_current_revision",
                    "request_id",
                    "input"
                ])
            ),
            json!({"task_id":example_id,"revision":1,"expected_current_revision":0,"request_id":"00000000-0000-4000-8000-000000000002","input":matrix_task_schema::example(),"choice_set":matrix_task_schema::example_choice_set(example_id)}),
        ),
        route!(
            "command",
            "engineering.matrix.verify",
            "verify_matrix_task",
            "Verify the saved Engineering Matrix facts against immutable evidence references.",
            "Requires an authenticated verifier native session bound to the workspace, an exact saved task revision and input digest, and one evidence reference per required fact. The verifier must differ from the owner who recorded the revision. Evidence validation is disabled by default.",
            "On successful evidence validation, appends a sealed verification record. The response is a bounded digest and per-fact status receipt without raw evidence content.",
            "On uncertainty, inspect the saved task revision; avoid retrying with changed evidence. Verification remains unavailable until a trusted evidence validator is configured.",
            object_schema(
                json!({
                    "task_id":uuid(),
                    "expected_revision":{"type":"integer","minimum":1},
                    "input_digest":{"type":"string","pattern":"^[0-9a-fA-F]{64}$"},
                    "evidence":{"type":"array","maxItems":1040,"items":object_schema(
                        json!({"fact_path":{"type":"string","minLength":1,"maxLength":512,"pattern":"^/"},"evidence_ref":{"type":"string","minLength":1,"maxLength":4096,"pattern":"\\S"}}),
                        json!(["fact_path","evidence_ref"])
                    )}
                }),
                json!(["task_id", "expected_revision", "input_digest", "evidence"])
            ),
            json!({"task_id":example_id,"expected_revision":1,"input_digest":"0".repeat(64),"evidence":[{"fact_path":"/mode","evidence_ref":"urn:evidence:example"}]}),
        ),
        route!(
            "query",
            "engineering.matrix.planning_effect.get",
            "get_matrix_planning_effect",
            "Read the exact selected Matrix choice and saved planning nodes for independent review.",
            "Requires an authenticated verifier native session, the selected candidate set ID, and the caller receipt request ID.",
            "Reads saved choice and mapped node bodies with a canonical effect digest; no planning state changes.",
            "Safe to repeat. Use the returned revision and digest for a separate attestation.",
            object_schema(
                json!({"candidate_set_id":uuid(),"caller_request_id":uuid()}),
                json!(["candidate_set_id", "caller_request_id"])
            ),
            json!({"candidate_set_id":example_id,"caller_request_id":"00000000-0000-4000-8000-000000000002"}),
        ),
        route!(
            "command",
            "engineering.matrix.planning_effect.verify",
            "verify_matrix_planning_effect",
            "Append an independent attestation of one saved Matrix planning effect.",
            "Requires an authenticated verifier native session separate from the Matrix owner and caller, exact current result revision and effect digest, and a bounded summary.",
            "Appends a match or rejection attestation; it does not mutate planning readiness or call Jev.",
            "Retry only with the same request_id and identical fields.",
            object_schema(
                json!({
                    "request_id":uuid(),"candidate_set_id":uuid(),"caller_request_id":uuid(),
                    "expected_result_revision":{"type":"integer","minimum":1},
                    "expected_effect_digest":{"type":"string","pattern":"^[0-9a-f]{64}$"},
                    "verdict":{"type":"string","enum":["matches","rejects"]},
                    "summary":{"type":"string","minLength":1,"maxLength":4096}
                }),
                json!([
                    "request_id",
                    "candidate_set_id",
                    "caller_request_id",
                    "expected_result_revision",
                    "expected_effect_digest",
                    "verdict",
                    "summary"
                ])
            ),
            json!({"request_id":"00000000-0000-4000-8000-000000000003","candidate_set_id":example_id,"caller_request_id":"00000000-0000-4000-8000-000000000002","expected_result_revision":1,"expected_effect_digest":"0".repeat(64),"verdict":"matches","summary":"Saved choice and mapped nodes match the intended plan"}),
        ),
        route!(
            "query",
            "pipeline.open_effect.get",
            "get_pipeline_open_effect",
            "Read the persisted effect of one explicit Slice open for an independent verifier.",
            "Requires a verifier session, the saved Slice ID and its open request ID.",
            "Returns saved Slice, caller receipt, Work node, source and Matrix binding with an effect digest; no state change.",
            "Safe to repeat; use the digest for a separate attestation.",
            object_schema(
                json!({"slice_id":uuid(),"open_request_id":uuid()}),
                json!(["slice_id", "open_request_id"])
            ),
            json!({"slice_id":example_id,"open_request_id":"00000000-0000-4000-8000-000000000002"}),
        ),
        route!(
            "command",
            "pipeline.open_effect.verify",
            "verify_pipeline_open_effect",
            "Append an independent match or rejection of one persisted Slice open.",
            "Requires a verifier session distinct from caller and Matrix owner, exact open request and effect digest.",
            "Appends an immutable observation; it does not complete a phase or call a provider.",
            "Retry with the same request_id and identical fields.",
            object_schema(
                json!({"request_id":uuid(),"slice_id":uuid(),"open_request_id":uuid(),"expected_effect_digest":{"type":"string","pattern":"^[0-9a-f]{64}$"},"verdict":{"type":"string","enum":["matches","rejects"]},"summary":{"type":"string","minLength":1,"maxLength":4096}}),
                json!([
                    "request_id",
                    "slice_id",
                    "open_request_id",
                    "expected_effect_digest",
                    "verdict",
                    "summary"
                ])
            ),
            json!({"request_id":"00000000-0000-4000-8000-000000000003","slice_id":example_id,"open_request_id":"00000000-0000-4000-8000-000000000002","expected_effect_digest":"0".repeat(64),"verdict":"matches","summary":"Persisted open matches the selected pipeline"}),
        ),
        route!(
            "query",
            "pipeline.phase_effect.get",
            "get_pipeline_phase_effect",
            "Read one saved completed phase attempt, output, selected verification plan obligation and backend evidence refs.",
            "Requires an independent verifier session, exact run ID and attempt ID.",
            "Returns a bound material digest; no phase, run or terminal state change.",
            "Safe to repeat; inspect the output before attesting.",
            object_schema(
                json!({"run_id":uuid(),"attempt_id":uuid()}),
                json!(["run_id", "attempt_id"])
            ),
            json!({"run_id":example_id,"attempt_id":"00000000-0000-4000-8000-000000000002"}),
        ),
        route!(
            "command",
            "pipeline.phase_effect.verify",
            "verify_pipeline_phase_effect",
            "Append a verifier-authored pass, fail or unknown observation for one saved completed phase attempt.",
            "Pass/fail require observation content and the exact saved output digest; without independent observation use unknown.",
            "Appends immutable evidence only; it does not advance a phase or run.",
            "Retry with the same request_id and identical fields.",
            object_schema(
                json!({"request_id":uuid(),"run_id":uuid(),"attempt_id":uuid(),"expected_effect_digest":{"type":"string","pattern":"^[0-9a-f]{64}$"},"verdict":{"type":"string","enum":["pass","fail","unknown"]},"observation":{"type":["string","null"],"minLength":32,"maxLength":16384},"observed_output_digest":{"type":["string","null"],"pattern":"^[0-9a-f]{64}$"},"summary":{"type":"string","minLength":1,"maxLength":4096}}),
                json!([
                    "request_id",
                    "run_id",
                    "attempt_id",
                    "expected_effect_digest",
                    "verdict",
                    "summary"
                ])
            ),
            json!({"request_id":"00000000-0000-4000-8000-000000000003","run_id":example_id,"attempt_id":"00000000-0000-4000-8000-000000000002","expected_effect_digest":"0".repeat(64),"verdict":"unknown","summary":"Independent observation unavailable"}),
        ),
        route!(
            "command",
            "engineering.matrix.disposition.record",
            "record_matrix_disposition",
            "Record the agent's explicit selected or blocked decision for an exact Matrix task revision and opportunity.",
            "Requires an authenticated native session, exact task/input/choice-set digests, current opportunity, matching basis and current advice token when basis is after_advice. The actor is derived from the session. A selected choice must be in the saved set; no automatic selection occurs.",
            "Appends an immutable typed disposition and returns its binding, digest and actor/session provenance without raw Jev content.",
            "Retry only with the same request_id and identical fields; use engineering.matrix.disposition.get to resolve an uncertain result.",
            object_schema(
                json!({
                    "request_id":uuid(),
                    "task_id":uuid(),
                    "expected_task_revision":{"type":"integer","minimum":1},
                    "expected_input_digest":{"type":"string","pattern":"^[0-9a-f]{64}$"},
                    "expected_choice_set_digest":{"type":["string","null"],"pattern":"^[0-9a-f]{64}$"},
                    "opportunity_id":uuid(),
                    "basis":{"type":"string","enum":["after_advice","no_call","manual"]},
                    "advice_id":{"type":["string","null"],"format":"uuid"},
                    "advice_digest":{"type":["string","null"],"pattern":"^[0-9a-f]{64}$"},
                    "decision":{"oneOf":[
                        object_schema(json!({"outcome":{"const":"selected"},"selected_choice_id":{"type":"string","minLength":1,"maxLength":4096}}),json!(["outcome","selected_choice_id"])),
                        object_schema(json!({"outcome":{"const":"blocked"},"blocked_reason":{"type":"string","minLength":1,"maxLength":4096}}),json!(["outcome","blocked_reason"]))
                    ]}
                }),
                json!([
                    "request_id",
                    "task_id",
                    "expected_task_revision",
                    "expected_input_digest",
                    "opportunity_id",
                    "basis",
                    "decision"
                ])
            ),
            json!({"request_id":"00000000-0000-4000-8000-000000000002","task_id":example_id,"expected_task_revision":1,"expected_input_digest":"0".repeat(64),"expected_choice_set_digest":null,"opportunity_id":"00000000-0000-4000-8000-000000000003","basis":"no_call","advice_id":null,"advice_digest":null,"decision":{"outcome":"blocked","blocked_reason":"Required facts remain unresolved"}}),
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
                json!({"request_id":uuid(),"input":text(),"task_context":super::planning_task_context()}),
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
                json!({"program_id":uuid(),"revision":{"type":"integer","minimum":1},"input_cursor":{"type":"integer","minimum":0},"name":nullable_text(),"intent":nullable_text(),"basis":nullable_text(),"boundaries":nullable_text(),"constraints":nullable_text(),"success":nullable_text(),"working_notes":nullable_text(),"pending_question":nullable_text(),"complete":{"type":"boolean","default":false},"consumed_knowledge":super::planning_manifest_guard()}),
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
                json!({"program_id":uuid(),"request_id":uuid(),"input":text(),"task_context":super::planning_task_context()}),
                json!(["program_id", "request_id", "input"]),
            ),
            json!({"program_id":example_id,"request_id":"00000000-0000-4000-8000-000000000002","input":"Complete original reply"}),
        ),
        route!(
            "command",
            "program.knowledge.refresh",
            "refresh_program_knowledge",
            "Refresh the immutable Program planning-knowledge context.",
            "Requires the current Program revision and saved input cursor. Omitted task_context preserves the prior normalized context; an explicit object replaces it completely.",
            "Atomically advances the Program revision and records one immutable manifest and exact replay receipt without adding user input or changing Program fields.",
            "Repeat with the same request_id and byte-identical parameters; changed parameters conflict.",
            object_schema(
                json!({"program_id":uuid(),"revision":{"type":"integer","minimum":1},"input_cursor":{"type":"integer","minimum":0},"request_id":uuid(),"task_context":super::planning_task_context()}),
                json!(["program_id", "revision", "input_cursor", "request_id"]),
            ),
            json!({"program_id":example_id,"revision":1,"input_cursor":0,"request_id":"00000000-0000-4000-8000-000000000003"}),
        ),
]
