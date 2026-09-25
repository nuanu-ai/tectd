use super::candidate_schema;
use super::catalog_support::{RouteSpec, uuid};
use crate::tools::object_schema;
use serde_json::{Value, json};

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

pub(super) fn routes(example_id: &str) -> Vec<RouteSpec> {
    vec![
        route!(
            "command",
            "pipeline.recommendation.prepare",
            "pipeline_recommendation_prepare",
            "Prepare a durable pipeline and verification recommendation opportunity for one current saved Work node.",
            "Requires an authenticated Owner with an active native session, exact current candidate-set and Work revisions, a saved Matrix disposition and independent match attestation. Preferences default to use_workspace; skip records a no-call.",
            "Records the opportunity, source context, and immutable eligible-choice manifest. Returns only the opportunity ID, state, reason, stable eligible IDs, and manifest digest. No provider call, pipeline execution, phase transition, or verification occurs.",
            "Repeat only the same request key and identical actor, session, revisions, and preferences. Changed material conflicts.",
            object_schema(
                json!({
                    "candidate_set_id":uuid(),
                    "expected_candidate_set_revision":{"type":"integer","minimum":2},
                    "work_node_id":uuid(),
                    "expected_work_node_revision":{"type":"integer","minimum":1},
                    "request_key":{"type":"string","minLength":1,"maxLength":256,"x-maxUtf8Bytes":256,"description":"One to 256 UTF-8 bytes, no NUL or leading or trailing Unicode whitespace."},
                    "session_preference":{"type":"string","enum":["use_workspace","skip"],"default":"use_workspace"},
                    "request_preference":{"type":"string","enum":["use_workspace","skip"],"default":"use_workspace"}
                }),
                json!([
                    "candidate_set_id",
                    "expected_candidate_set_revision",
                    "work_node_id",
                    "expected_work_node_revision",
                    "request_key"
                ]),
            ),
            json!({"candidate_set_id":example_id,"expected_candidate_set_revision":2,"work_node_id":example_id,"expected_work_node_revision":1,"request_key":"work-1"}),
        ),
        route!(
            "command",
            "pipeline.recommendation.run",
            "pipeline_recommendation_run",
            "Attempt one guarded pipeline recommendation for an exact prepared opportunity.",
            "Requires the authenticated Owner and original native session, a current prepared opportunity, unchanged advisory configuration and current saved Work, Matrix, and manifest bindings. A no-call opportunity returns its reason without a send.",
            "Commits one dispatch start before a provider attempt, durably seals returned response bytes and digest, and returns only a validated ranking or abstention. It does not open a Slice, transition a phase, or establish verification.",
            "Never retry a provider attempt after uncertain send. Inspect workspace.advisory.audit after uncertainty; replay cannot create another dispatch.",
            object_schema(json!({"opportunity_id":uuid()}), json!(["opportunity_id"])),
            json!({"opportunity_id":example_id}),
        ),
        route!(
            "command",
            "pipeline.recommendation.disposition",
            "pipeline_recommendation_disposition",
            "Record the Owner's explicit decision on one saved pipeline recommendation.",
            "Requires the original authenticated Owner session, exact opportunity, current Work revision and immutable manifest digest. Ranked advice can be accepted, rejected, or superseded by the deterministic choice; no-call and abstention allow only the deterministic choice.",
            "Stores one immutable planning disposition. It does not open a Slice, execute a pipeline, transition a phase, or establish verification.",
            "An identical request replays its receipt; changed material conflicts. A stale Work, Matrix, source, catalogue, configuration, or manifest binding fails.",
            object_schema(
                json!({"request_id":uuid(),"opportunity_id":uuid(),
                    "expected_work_revision":{"type":"integer","minimum":1},
                    "manifest_digest":{"type":"string","pattern":"^[0-9a-f]{64}$"},
                    "action":{"type":"string","enum":["accept_recommendation","reject_recommendation","use_deterministic_choice"]},
                    "rationale":{"type":"string","minLength":1,"maxLength":4096}}),
                json!([
                    "request_id",
                    "opportunity_id",
                    "expected_work_revision",
                    "manifest_digest",
                    "action",
                    "rationale"
                ]),
            ),
            json!({"request_id":example_id,"opportunity_id":example_id,"expected_work_revision":1,
                "manifest_digest":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                "action":"use_deterministic_choice","rationale":"Use the saved Work choice"}),
        ),
        route!(
            "command",
            "engineering.advisory.request",
            "request_engineering_advisory",
            "Record an optional Engineering Matrix advisory opportunity for one exact saved task revision.",
            "Requires an authenticated open native session, workspace membership, the current positive task revision, and a request key unique to this opportunity. The optional session and request preferences each default to use_workspace; skip records a no-call. A saved choice set with fewer than two eligible candidates is not applicable.",
            "Records a terminal no_call opportunity with its reason, task revision, optional choice-set digest, configuration revision, and material digest. The provider is never called; no advice, choice, release, or approval is established.",
            "Repeat the same request key with identical task revision, actor/session, and preferences. Changed material conflicts; inspect engineering.advisory.get after uncertainty.",
            object_schema(
                json!({
                    "task_id":uuid(),
                    "expected_task_revision":{"type":"integer","minimum":1},
                    "request_key":{"type":"string","minLength":1,"maxLength":256,"x-maxUtf8Bytes":256,"description":"One to 256 UTF-8 bytes, no NUL, and no leading or trailing Unicode whitespace. Host validation enforces byte and trim limits."},
                    "session_preference":{"type":"string","enum":["use_workspace","skip"],"default":"use_workspace"},
                    "request_preference":{"type":"string","enum":["use_workspace","skip"],"default":"use_workspace"}
                }),
                json!(["task_id", "expected_task_revision", "request_key"]),
            ),
            json!({"task_id":example_id,"expected_task_revision":1,"request_key":"matrix-task-1","request_preference":"use_workspace"}),
        ),
        route!(
            "query",
            "engineering.advisory.get",
            "get_engineering_advisory",
            "Read an Engineering Matrix advisory receipt and any guarded advice still current for the exact task and request key.",
            "Requires an authenticated open native session in the receipt's workspace. The receipt must target the exact Matrix task and request key.",
            "Returns the saved receipt. For advised opportunities only, current_advice contains the persisted ranking or abstention, advice and dispatch IDs, provider identity, and binding digests after current task, configuration, latest verification, and evidence revalidation. Stale advice is omitted. Provider response bytes are never returned. No choice or release is established.",
            "Safe to repeat; a missing or mismatched receipt returns not_found.",
            object_schema(
                json!({"task_id":uuid(),"request_key":{"type":"string","minLength":1,"maxLength":256,"x-maxUtf8Bytes":256,"description":"One to 256 UTF-8 bytes, no NUL, and no leading or trailing Unicode whitespace. Host validation enforces byte and trim limits."}}),
                json!(["task_id", "request_key"]),
            ),
            json!({"task_id":example_id,"request_key":"matrix-task-1"}),
        ),
        route!(
            "command",
            "scope.advisory.request",
            "scope_advisory_request",
            "Request optional Scope-decomposition advice for one candidate set using complete agent-authored alternatives.",
            "Requires an authenticated open native session and accessible candidate set. Workspace disabled or request skip records an auditable no-call. An active request requires the full authored set at the expected candidate revision; session preference is bound by the host, not caller-supplied.",
            "Records an opportunity; with production adapters disabled it cannot contact Jev. When separately enabled, the registered orchestration audits each dispatch before any provider attempt. It does not open a Scope or apply advice.",
            "Repeat only with the same request_id and identical authored material; changed material conflicts. Inspect candidate.advisory.get/audit after uncertainty.",
            scope_advisory_request_schema(),
            json!({"request_id":example_id,"candidate_set_id":example_id,"request_preference":"skip"}),
        ),
        route!(
            "command",
            "scope.anti_bloat.prepare",
            "anti_bloat_prepare",
            "Prepare an auditable source-bound anti-bloat review for one exact candidate-set revision.",
            "Requires the owning open native session, authoritative frozen source binding, and current candidate-set revision.",
            "Saves a deterministic local review; disabled, skip, and no-eligible states never call a provider.",
            "One-shot prepare creates a new review ID on each call; retain the returned ID for recovery.",
            object_schema(
                json!({"candidate_set_id":uuid(),"expected_revision":{"type":"integer","minimum":1},"request_preference":{"type":"string","enum":["use_workspace","skip"]}}),
                json!(["candidate_set_id", "expected_revision"])
            ),
            json!({"candidate_set_id":example_id,"expected_revision":3,"request_preference":"skip"}),
        ),
        route!(
            "query",
            "scope.anti_bloat.get",
            "anti_bloat_get",
            "Read one saved anti-bloat review and its send state.",
            "Requires the original owner in the review workspace and the review ID.",
            "Returns graph digests, findings, state and ranked IDs without raw transport bytes.",
            "Safe to repeat by review ID.",
            object_schema(json!({"review_id":uuid()}), json!(["review_id"])),
            json!({"review_id":example_id}),
        ),
        route!(
            "command",
            "scope.anti_bloat.run",
            "anti_bloat_run",
            "Run the one-use ranking attempt for a prepared anti-bloat review.",
            "Requires the original owner, prepared state and current frozen source/plan binding. Provider transport is disabled by default.",
            "Commits request bytes and send fence before transport; commits raw response before interpretation. Failure remains send_unknown.",
            "Repeat by review ID to read terminal state; sending and send_unknown are never resent.",
            object_schema(json!({"review_id":uuid()}), json!(["review_id"])),
            json!({"review_id":example_id}),
        ),
        route!(
            "command",
            "scope.anti_bloat.apply",
            "anti_bloat_apply",
            "Apply one explicit candidate removal after source-bound preservation checking.",
            "Requires original owner, exact review/finding, narrow disposition, and caller-authored candidate delta with CAS revision and idempotency key.",
            "Checks preservation and applies the caller delta atomically; provider output never authors a mutation.",
            "Repeat only with the same review, finding and exact delta idempotency key; changed input conflicts.",
            object_schema(
                json!({"review_id":uuid(),"finding_id":{"type":"string","pattern":"^[0-9a-f]{64}$"},"disposition":{"type":"string","enum":["narrow"]},"delta":candidate_schema::delta_apply()}),
                json!(["review_id", "finding_id", "disposition", "delta"])
            ),
            json!({"review_id":example_id,"finding_id":"a".repeat(64),"disposition":"narrow","delta":{"candidate_set_id":example_id,"expected_revision":3,"idempotency_key":"narrow-1","operations":[{"operation":"candidate.remove","candidate_id":example_id,"expected_revision":1}]}}),
        ),
        route!(
            "command",
            "scope.advisory.disposition",
            "scope_advisory_disposition",
            "Explicitly accept one stored Scope alternative or reject all stored advice alternatives.",
            "Requires the original authenticated session, exact opportunity, candidate set and guarded advice IDs, the current disposition revision, and one item for every eligible alternative. Accept selects one eligible alternative; reject_all selects none; supersede_with_deterministic_choice selects the manifest baseline. Unknown, duplicate or non-manifest IDs fail validation.",
            "Persists an audited disposition through compare-and-set. It does not authorize a caller, mutate Scope, or contact Jev.",
            "Repeat an identical request_id and payload for replay. A changed payload conflicts; a competing expected revision is stale.",
            scope_advisory_disposition_schema(),
            json!({"opportunity_id":example_id,"candidate_set_id":example_id,"request_id":example_id,"advice_id":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","expected_revision":0,"action":"reject_all","items":[{"alternative_id":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","state":"not_selected"}],"rationale":"Do not use this advice"}),
        ),
        route!(
            "query",
            "workspace.advisory.config",
            "get_advisory_config",
            "Read the workspace Jev advisory mode and its revision.",
            "Requires an authenticated open native session. A missing row is the explicit default: disabled at revision 0.",
            "Reads workspace-scoped advisory configuration only; it never contacts a provider.",
            "Safe to repeat.",
            object_schema(json!({}), json!([])),
            json!({}),
        ),
        route!(
            "command",
            "workspace.advisory.configure",
            "configure_advisory",
            "Set workspace advisory mode and non-secret provider/model references with owner-only compare-and-set revision.",
            "Requires an authenticated open native session whose principal is the tenant owner. The first write expects revision 0; later writes must name the current revision.",
            "Appends config history and changes only future dispatch authorization. In-flight sends are not promised to cancel.",
            "Retry only with the same expected revision and mode; a stale revision must be re-read.",
            advisory_config_schema(),
            json!({"expected_revision":0,"mode":"optional","provider_profile_ref":{"id":"jev-production"},"model_configuration":{"model":"jev-advisory-v1"}}),
        ),
        route!(
            "query",
            "workspace.advisory.audit",
            "workspace_advisory_audit",
            "Read a stable page of workspace advisory opportunities, dispatch facts and exact basic aggregates.",
            "Requires an authenticated open native session in the workspace. Optional scope UUID and closed-enum filters are tenant-bound; after is the last opportunity ID returned by the preceding page.",
            "Returns metadata and byte counts, never request/response bodies or credentials. Unknown send/token facts remain explicit, and future advice/disposition/caller/verifier links are null.",
            "Safe to repeat with the same filters and cursor.",
            advisory_audit_schema(false),
            json!({"limit":50}),
        ),
        route!(
            "query",
            "scope.advisory.get",
            "scope_advisory_get",
            "Read one exact scope advisory opportunity and all of its ordered dispatch facts.",
            "Requires an authenticated open native session and an existing scope in that workspace; the opportunity must belong to that exact scope.",
            "Returns metadata, retry lineage, nullable measurements and raw-response reference only. It does not return raw provider bodies and does not infer future links.",
            "Safe to repeat.",
            object_schema(
                json!({"scope_id":uuid(),"opportunity_id":uuid()}),
                json!(["scope_id", "opportunity_id"]),
            ),
            json!({"scope_id":example_id,"opportunity_id":example_id}),
        ),
        route!(
            "query",
            "scope.advisory.card",
            "scope_advisory_card",
            "Compose the versioned Engineering Matrix cards for one exact saved task revision.",
            "Requires an authenticated open native session, workspace membership, a current task UUID, and its exact positive revision. detail defaults to summary; full requires one card_id from the returned catalogue.",
            "Returns catalogue version, task identity and revision, every applicable card ID and summary, and unresolved evidence. Full detail adds the selected card body. This read never contacts Jev or changes an advisory opportunity.",
            "Safe to repeat for the same task revision; a changed revision is stale.",
            matrix_card_schema(),
            json!({"task_id":example_id,"expected_task_revision":1}),
        ),
        route!(
            "query",
            "scope.advisory.audit",
            "scope_advisory_audit",
            "Read a stable filtered page of advisory opportunities and dispatch facts for one exact scope.",
            "Requires an authenticated open native session and an existing scope in that workspace. Optional filters are closed enums; after is the preceding page's last opportunity ID.",
            "Returns exact no-call reasons, send uncertainty and aggregates without raw bodies or inferred disposition/caller/verifier outcomes.",
            "Safe to repeat with the same filters and cursor.",
            advisory_audit_schema(true),
            json!({"scope_id":example_id,"limit":50}),
        ),
        route!(
            "command",
            "candidate.advisory.verify",
            "candidate_advisory_verify",
            "Independently verify the recorded selected candidate save from server-held evidence.",
            "Requires a distinct enrolled verifier principal with an open session and workspace membership. The caller supplies only the exact saved target and request IDs; the host binds verifier identity and session.",
            "Records a server-computed passed or failed observation with independent qualification. It grants no approval or current acceptance and changes no candidate or caller material.",
            "Repeat an identical request_id and target for replay. Changed target or session conflicts; inspect candidate.advisory.get/audit after uncertainty.",
            object_schema(
                json!({
                    "request_id":uuid(),
                    "opportunity_id":uuid(),
                    "candidate_set_id":uuid(),
                    "caller_link_id":uuid(),
                    "caller_receipt_request_id":uuid(),
                    "target_revision":{"type":"integer","minimum":1}
                }),
                json!([
                    "request_id",
                    "opportunity_id",
                    "candidate_set_id",
                    "caller_link_id",
                    "caller_receipt_request_id",
                    "target_revision"
                ]),
            ),
            json!({"request_id":example_id,"opportunity_id":example_id,"candidate_set_id":example_id,"caller_link_id":example_id,"caller_receipt_request_id":example_id,"target_revision":1}),
        ),
        route!(
            "query",
            "candidate.advisory.get",
            "candidate_advisory_get",
            "Read one candidate-set advisory opportunity and its ordered dispatch facts.",
            "Requires an authenticated open owner or verifier session and a candidate set in the workspace; the opportunity must target that exact candidate set.",
            "Returns metadata and dispatch facts. For an owner reading advised scope decomposition, scope_decomposition version 1 contains the validated persisted manifest and guarded advice, including eligible IDs, baseline, ranked IDs, and typed answers; verifier and no-call responses omit it.",
            "Safe to repeat.",
            object_schema(
                json!({"candidate_set_id":uuid(),"opportunity_id":uuid()}),
                json!(["candidate_set_id", "opportunity_id"])
            ),
            json!({"candidate_set_id":example_id,"opportunity_id":example_id}),
        ),
        route!(
            "query",
            "candidate.advisory.audit",
            "candidate_advisory_audit",
            "Read a filtered page of opportunities, dispatch facts and aggregates for one candidate set.",
            "Requires an authenticated open owner or verifier session and a candidate set in the workspace; after is the preceding page's last opportunity ID.",
            "Returns exact counts and no-call reasons without raw provider bodies.",
            "Safe to repeat with the same filters and cursor.",
            advisory_candidate_audit_schema(),
            json!({"candidate_set_id":example_id,"limit":50}),
        ),
    ]
}

fn matrix_card_schema() -> Value {
    let mut schema = object_schema(
        json!({
            "task_id":uuid(),
            "expected_task_revision":{"type":"integer","minimum":1},
            "card_id":{"type":"string","enum":["EM02-SCOPE@0.1","EM02-PROTECT@0.1","EM02-OPERATE@0.1","EM02-CAPACITY@0.1","EM02-HOTFIX@0.1"]},
            "detail":{"type":"string","enum":["summary","full"],"default":"summary"}
        }),
        json!(["task_id", "expected_task_revision"]),
    );
    schema["if"] = json!({"properties":{"detail":{"const":"full"}},"required":["detail"]});
    schema["then"] = json!({"required":["card_id"]});
    schema
}

fn scope_advisory_request_schema() -> Value {
    let local_key =
        json!({"type":"string","minLength":1,"maxLength":64,"pattern":"^[A-Za-z0-9._-]+$"});
    let alternative = object_schema(
        json!({
            "key":local_key,
            "kind":{"type":"string","enum":["cohesive","partitioned"]},
            "draft":super::candidate_schema::authored_draft(),
            "covered_source_ref_ids":{"type":"array","items":uuid(),"minItems":1,"uniqueItems":true}
        }),
        json!(["key", "kind", "draft", "covered_source_ref_ids"]),
    );
    let authored = object_schema(
        json!({
            "expected_candidate_set_revision":{"type":"integer","minimum":1},
            "baseline_key":{"type":"string","minLength":1,"maxLength":64,"pattern":"^[A-Za-z0-9._-]+$"},
            "alternatives":{"type":"array","items":alternative,"minItems":1,"maxItems":100}
        }),
        json!([
            "expected_candidate_set_revision",
            "baseline_key",
            "alternatives"
        ]),
    );
    object_schema(
        json!({
            "request_id":uuid(),
            "candidate_set_id":uuid(),
            "request_preference":{"type":"string","enum":["use_workspace","skip"]},
            "authored_scope_set":authored
        }),
        json!(["request_id", "candidate_set_id"]),
    )
}

fn scope_advisory_disposition_schema() -> Value {
    let digest = json!({"type":"string","pattern":"^[0-9a-f]{64}$"});
    object_schema(
        json!({
            "opportunity_id":uuid(),
            "candidate_set_id":uuid(),
            "request_id":uuid(),
            "advice_id":digest,
            "expected_revision":{"type":"integer","minimum":0},
            "action":{"type":"string","enum":["accept","reject_all","supersede_with_deterministic_choice"]},
            "selected_id":digest,
            "items":{"type":"array","minItems":1,"maxItems":100,"items":object_schema(json!({"alternative_id":digest,"state":{"type":"string","enum":["selected","not_selected"]}}),json!(["alternative_id","state"]))},
            "rationale":{"type":"string","minLength":1,"maxLength":4096}
        }),
        json!([
            "opportunity_id",
            "candidate_set_id",
            "request_id",
            "advice_id",
            "expected_revision",
            "action",
            "items",
            "rationale"
        ]),
    )
}

fn advisory_candidate_audit_schema() -> Value {
    let mut schema = advisory_audit_schema(false);
    let properties = schema["properties"]
        .as_object_mut()
        .expect("object schema properties");
    properties.remove("scope_id");
    properties.insert("candidate_set_id".into(), uuid());
    schema["required"] = json!(["candidate_set_id", "limit"]);
    schema
}

fn advisory_config_schema() -> Value {
    let provider_profile_ref =
        object_schema(json!({"id": advisory_identifier_schema()}), json!(["id"]));
    let model_configuration = object_schema(
        json!({"model": advisory_identifier_schema()}),
        json!(["model"]),
    );
    let mut schema = object_schema(
        json!({
            "expected_revision":{"type":"integer","minimum":0},
            "mode":{"type":"string","enum":["disabled","optional"]},
            "provider_profile_ref":{"oneOf":[provider_profile_ref,{"type":"null"}]},
            "model_configuration":{"oneOf":[model_configuration,{"type":"null"}]}
        }),
        json!(["expected_revision", "mode"]),
    );
    schema["oneOf"] = json!([
        {"required":["provider_profile_ref", "model_configuration"]},
        {"not":{"anyOf":[
            {"required":["provider_profile_ref"]},
            {"required":["model_configuration"]}
        ]}}
    ]);
    schema
}

fn advisory_identifier_schema() -> Value {
    json!({
        "type":"string",
        "minLength":1,
        "maxLength":256,
        "pattern":r"^[^\s\u0000](?:[^\u0000]*[^\s\u0000])?$"
    })
}

fn advisory_audit_schema(scope: bool) -> Value {
    let mut properties = serde_json::Map::from_iter([
        (
            "limit".into(),
            json!({"type":"integer","minimum":1,"maximum":100}),
        ),
        ("after".into(), uuid()),
        (
            "capability".into(),
            json!({"type":"string","enum":["scope_decomposition","engineering_profile","pipeline_recommendation","anti_bloat","model_routing"]}),
        ),
        (
            "decision_point".into(),
            json!({"type":"string","enum":["scope.decomposition.before_selection","engineering.profile.before_selection"]}),
        ),
        (
            "reason".into(),
            json!({"type":"string","enum":["workspace_disabled","session_skip","request_skip","choice_set_not_applicable","deterministic_input_invalid","capability_unavailable","provider_unconfigured","budget_policy_invalid","configuration_changed","dispatch_authorized","provider_response","provider_failure","send_unknown"]}),
        ),
        (
            "state".into(),
            json!({"type":"string","enum":["prepared","no_call","awaiting_response","advised","invalidated","failed","unresolved"]}),
        ),
    ]);
    let required = if scope {
        properties.insert("scope_id".into(), uuid());
        json!(["scope_id", "limit"])
    } else {
        properties.insert("scope_id".into(), uuid());
        json!(["limit"])
    };
    object_schema(Value::Object(properties), required)
}
