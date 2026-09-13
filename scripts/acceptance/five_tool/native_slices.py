"""Deterministic native Scope/Slice flow inside the existing five-tool acceptance."""
from __future__ import annotations
import json, uuid
from typing import Any, Callable
import scope_candidates

PIPELINES = {
    "slice.lightweight-tdd-development", "slice.full-design-to-execution",
    "slice.debug-root-cause", "slice.operational-preparation", "slice.operational-execution",
    "slice.research-to-durable-knowledge", "slice.custom-procedure-capture",
}
RULES = {
    "vertical-provable-slices", "no-unrequested-or-unauthorized-work",
    "autonomous-local-technical-decisions", "no-product-test-harness-work",
}

def ok(call, tool: str, route: str, params: dict[str, Any]) -> dict[str, Any]:
    payload, failed = call(tool, {"route": route, "params": params})
    if failed:
        raise AssertionError(f"{route} failed: {payload.get('error', {}).get('code')}")
    return payload

def variant(payload: dict[str, Any], name: str) -> dict[str, Any]:
    value = payload.get(name)
    if not isinstance(value, dict): raise AssertionError(f"missing {name} outcome")
    return value

def action_params(payload: dict[str, Any], route: str) -> dict[str, Any]:
    for action in payload.get("actions", []):
        if action.get("arguments", {}).get("route") == route:
            params = action["arguments"].get("params")
            if isinstance(params, dict): return json.loads(json.dumps(params))
    raise AssertionError(f"server omitted {route} action")

def source_candidate(call, source_path: str) -> dict[str, Any]:
    program = scope_candidates.prepare_open_program(call, source_path)
    begun = ok(call, "command", "scope.candidates.begin", {
        "request_id": str(uuid.uuid4()), "program_id": program["program_id"],
        "program_revision": program["program_revision"], "boundary": "ongoing",
        "input": "Diagnose the incorrect notification preview, then select its smallest correction.",
    })
    context, candidate_set = begun["context"], begun["context"]["candidate_set"]
    inputs = ok(call, "query", "scope.candidates.context", {
        "candidate_set_id": candidate_set["id"], "view": "inputs", "limit": 25,
    })
    ref = inputs["items"][0]["input"]["source_ref_id"]
    saved = ok(call, "command", "scope.candidates.save", {
        "kind": "draft", "candidate_set_id": candidate_set["id"],
        "revision": candidate_set["revision"], "snapshot_id": context["snapshot"]["id"],
        "input_cursor": candidate_set["latest_input"], "request_id": str(uuid.uuid4()),
        "draft": {"boundary":"ongoing", "goals":[{
            "identity":{"local":"goal"}, "text":"Explain the preview deviation and bound its correction.",
            "source_ref_id":ref, "resolution":{"kind":"candidate","reference":{"local":"scope"}},
        }], "evidence":[], "candidates":[{
            "identity":{"local":"scope"}, "title":"Preview diagnosis and correction decision",
            "outcome":"The cause is demonstrated and the smallest correction path selected.",
            "trigger":"Preview differs from saved settings.",
            "delivered_behavior":"A cause and bounded follow-up decision are available.",
            "proof":"Native planning preserves the result and decision.",
            "includes":["diagnosis","correction decision"], "excludes":["SMS","deployment"],
            "dependencies":[], "coverage_goals":[{"local":"goal"}], "evidence":[],
        }], "blockers":[], "protected_changes":[]},
    })
    candidate = saved["draft"]["candidates"][0]
    reviewed = ok(call, "command", "scope.candidates.save", {
        "kind":"review", "candidate_set_id":candidate_set["id"],
        "revision":saved["context"]["candidate_set"]["revision"],
        "snapshot_id":context["snapshot"]["id"],
        "input_cursor":saved["context"]["candidate_set"]["input_cursor"],
        "request_id":str(uuid.uuid4()), "review":{
            "verdict":"ready", "summary":"Bounded, vertical, traceable and ready.", "findings":[],
            "candidate_decisions":[{"candidate_id":candidate["id"], "decision":"accept",
                                    "rationale":"One coherent Scope."}],
        },
    })
    if reviewed["context"]["candidate_set"]["status"] != "ready":
        raise AssertionError("source candidate did not become ready")
    return {"context":reviewed["context"], "candidate":candidate}

def existing_work(node: dict[str, Any]) -> dict[str, Any]:
    return {"kind":"work", "identity":{"candidate_id":node["id"],"revision":node["revision"]},
            "title":node["title"], "outcome":node["outcome"], "includes":node["includes"],
            "excludes":node["excludes"], "dependencies":[], "proof":node["proof"],
            "pipeline":node["pipeline"], "pipeline_reason":node["pipeline_reason"],
            "source_result_ids":node["source_result_ids"]}

def save_plan(call, context, draft):
    return ok(call, "command", "slice.candidates.save", {
        "kind":"draft", "scope_id":context["scope"]["id"],
        "candidate_set_id":context["candidate_set"]["id"],
        "revision":context["candidate_set"]["revision"], "snapshot_id":context["snapshot"]["id"],
        "input_cursor":context["candidate_set"]["input_cursor"], "request_id":str(uuid.uuid4()),
        "draft":draft,
    })

def review_plan(call, context, summary):
    return ok(call, "command", "slice.candidates.save", {
        "kind":"review", "scope_id":context["scope"]["id"],
        "candidate_set_id":context["candidate_set"]["id"],
        "revision":context["candidate_set"]["revision"], "snapshot_id":context["snapshot"]["id"],
        "input_cursor":context["candidate_set"]["input_cursor"], "request_id":str(uuid.uuid4()),
        "review":{"verdict":"ready", "summary":summary, "findings":[]},
    })

def open_params(context, candidate, request_id=None):
    return {"request_id":request_id or str(uuid.uuid4()), "scope_id":context["scope"]["id"],
            "scope_revision":context["scope"]["revision"],
            "candidate_set_id":context["candidate_set"]["id"],
            "candidate_set_revision":context["candidate_set"]["revision"],
            "candidate_snapshot_id":context["snapshot"]["id"],
            "candidate_id":candidate["id"], "candidate_revision":candidate["revision"]}

def run(call: Callable, source_path: str, check: Callable) -> dict[str, Any]:
    source = source_candidate(call, source_path)
    catalogue = ok(call, "query", "slice.pipelines", {})
    entries = catalogue.get("pipelines", []); ids = {entry.get("kind") for entry in entries}
    check("native Slice catalogue exposes exactly seven provisional non-executable stubs",
          ids == PIPELINES and "slice.hybrid-implementation-operation" not in json.dumps(catalogue)
          and catalogue.get("executable") is False
          and all(x.get("implementation_status")=="stub" and x.get("description_status")=="provisional"
                  and x.get("refinement_required") is True for x in entries),
          {"pipeline_ids":sorted(ids),"executable":catalogue.get("executable")})

    sc, cand = source["context"], source["candidate"]
    scope_request = {"request_id":str(uuid.uuid4()), "candidate_set_id":sc["candidate_set"]["id"],
        "candidate_set_revision":sc["candidate_set"]["revision"],
        "candidate_snapshot_id":sc["snapshot"]["id"], "candidate_id":cand["id"],
        "candidate_revision":cand["revision"]}
    scope_opened = ok(call,"command","scope.open",scope_request)
    created = variant(scope_opened,"created")
    replay = variant(ok(call,"command","scope.open",scope_request),"replay")
    check("scope.open creates Scope plus initial snapshot with exact replay",
          created==replay and created["scope"]["source_candidate_id"]==cand["id"]
          and created["planning"]["snapshot"]["sequence"]==1,
          {"scope_id":created["scope"]["id"]})
    planning=created["planning"]; rules=planning["snapshot"]["rules"]
    rule_ids={rule.get("id") for rule in rules}
    check("initial Slice design carries all four full rule bodies",
          rule_ids==RULES and all(len(rule.get("text","").strip())>100 for rule in rules),
          {"rule_ids":sorted(rule_ids)})

    initial={"coverage_summary":"Diagnosis followed by its decision.","nodes":[
        {"kind":"work","identity":{"local":"diagnose"},"title":"Demonstrate preview root cause",
         "outcome":"The preview deviation has a demonstrated cause and correction direction.",
         "includes":["reproduction","cause","direction"],"excludes":["fix","deployment"],
         "dependencies":[],"proof":["A causal observation is recorded."],
         "pipeline":"slice.debug-root-cause","pipeline_reason":"The cause is unknown.","source_result_ids":[]},
        {"kind":"decision","identity":{"local":"choose"},"title":"Choose correction path",
         "question":"Is Lightweight sufficient or is justified Full required?",
         "resolution_criteria":["The diagnosis identifies boundaries and irreducible complexity."],
         "dependencies":[{"local":"diagnose"}],"source_result_ids":[]},
    ],"supersessions":[]}
    offered_save=action_params(scope_opened,"slice.candidates.save")
    offered_save["request_id"]=str(uuid.uuid4()); offered_save["draft"]=initial
    saved=ok(call,"command","slice.candidates.save",offered_save)
    debug=next(n for n in saved["draft"]["nodes"] if n["kind"]=="work")
    decision=next(n for n in saved["draft"]["nodes"] if n["kind"]=="decision")
    reviewed=review_plan(call,saved,"The complete initial graph preserves its material decision.")
    check("initial plan persists Debug work and an unresolved Decision",
          reviewed["candidate_set"]["status"]=="ready"
          and {n["kind"] for n in reviewed["draft"]["nodes"]}=={"work","decision"},
          {"candidate_set_id":reviewed["candidate_set"]["id"]})

    open_request=open_params(reviewed,debug)
    opened=ok(call,"command","slice.open",open_request); slice_=variant(opened,"created")
    slice_read=ok(call,"query","slice.context",action_params(opened,"slice.context"))
    check("slice.open starts no stub execution and reinjects no design rules",
          slice_["pipeline"]=="slice.debug-root-cause" and slice_["pipeline_status"]=="stub"
          and slice_["execution_claimed"] is False and "rules" not in json.dumps(opened)
          and all(a.get("tool")!="execute" for a in opened.get("actions",[]))
          and slice_read.get("id")==slice_["id"] and slice_read.get("pipeline")==slice_["pipeline"],
          {"slice_id":slice_["id"],"actions":opened.get("actions",[])})

    result_payload=ok(call,"command","slice.result.record",{
        "request_id":str(uuid.uuid4()),"scope_id":reviewed["scope"]["id"],
        "slice_id":slice_["id"],"slice_revision":slice_["revision"],"outcome":"completed",
        "summary":"The deterministic fixture recorded a bounded diagnosis result.",
        "evidence":[{"kind":"fixture_observation","reference":"native deterministic API call sequence",
                     "observation":"Caller-supplied result persisted and returned through TectD."}],
        "scope_impact":"The correction decision can be resolved.",
        "remaining_work":"Refresh and review a bounded correction candidate."})
    result_created=variant(result_payload,"created"); result=result_created["result"]
    stale=result_created["context"]
    check("result is externally_reported and makes future planning stale",
          result["provenance"]=="externally_reported" and result["outcome"]=="completed"
          and bool(result["evidence"]) and "planning_inputs" in stale["stale_reasons"],
          {"result_id":result["id"],"provenance":result["provenance"],"stale":stale["stale_reasons"]})
    replay_slice=variant(ok(call,"command","slice.open",open_request),"replay")
    check("exact prior slice.open still replays after later result",replay_slice==slice_,{"slice_id":slice_["id"]})
    stale_request=open_params(reviewed,decision)
    rejected,failed=call("command",{"route":"slice.open","params":stale_request})
    check("new open against stale pre-result plan is rejected",
          failed and rejected.get("error",{}).get("code") in {"stale_context","stale_revision"},
          {"error_code":rejected.get("error",{}).get("code")})

    refresh_params=action_params(result_payload,"slice.candidates.refresh")
    refresh_params["request_id"]=str(uuid.uuid4())
    refreshed=ok(call,"command","slice.candidates.refresh",refresh_params)
    rr={r.get("id") for r in refreshed["snapshot"]["rules"]}
    check("result refresh captures result and all four design rules",
          not refreshed["stale_reasons"] and result["id"] in refreshed["snapshot"]["result_ids"] and rr==RULES,
          {"result_ids":refreshed["snapshot"]["result_ids"],"rule_ids":sorted(rr)})

    revised=save_plan(call,refreshed,{"coverage_summary":"Result resolves decision to bounded correction.","nodes":[
        existing_work(debug),
        {"kind":"work","identity":{"local":"correction"},"title":"Correct preview derivation",
         "outcome":"Preview reflects saved recipient and cadence settings.",
         "includes":["bounded correction","regression proof"],"excludes":["redesign","deployment"],
         "dependencies":[{"candidate_id":debug["id"],"revision":debug["revision"]}],
         "proof":["Focused regression and existing preview tests pass."],
         "pipeline":"slice.lightweight-tdd-development",
         "pipeline_reason":"The result isolates one small vertical correction.",
         "source_result_ids":[result["id"]]},
    ],"supersessions":[{"candidate_id":decision["id"],"revision":decision["revision"],
        "reason":"Diagnosis resolves the correction-path question.","replacements":[{"local":"correction"}]}]})
    light=next(n for n in revised["draft"]["nodes"] if n["pipeline"]=="slice.lightweight-tdd-development")
    final=review_plan(call,revised,"Result-backed Lightweight successor is bounded and history explicit.")
    follow=variant(ok(call,"command","slice.open",open_params(final,light)),"created")
    check("externally reported result supports Decision-to-Lightweight continuation",
          follow["pipeline"]=="slice.lightweight-tdd-development" and follow["execution_claimed"] is False,
          {"slice_id":follow["id"],"source_result_id":result["id"]})
    blocked_payload=ok(call,"command","slice.result.record",{
        "request_id":str(uuid.uuid4()),"scope_id":final["scope"]["id"],"slice_id":follow["id"],
        "slice_revision":follow["revision"],"outcome":"blocked","summary":"Caller reports a bounded blocker.",
        "evidence":[{"kind":"fixture_observation","reference":"native deterministic API call sequence",
                     "observation":"A blocked external result was accepted once."}],
        "scope_impact":"Future correction work requires review.","remaining_work":"Refresh the affected plan."})
    blocked=variant(blocked_payload,"created")
    offered_refresh=action_params(blocked_payload,"slice.candidates.refresh")
    check("a blocked external result leads to refresh instead of another result prompt",
          blocked["result"]["outcome"]=="blocked"
          and offered_refresh["candidate_set_id"]==final["candidate_set"]["id"]
          and all(a.get("arguments",{}).get("route")!="slice.result.record"
                  for a in blocked_payload.get("actions",[])),
          {"result_id":blocked["result"]["id"],"next_route":"slice.candidates.refresh"})
    blocked_slice=ok(call,"query","slice.context",{"slice_id":follow["id"]})
    completed_payload=ok(call,"command","slice.result.record",{
        "request_id":str(uuid.uuid4()),"scope_id":final["scope"]["id"],"slice_id":follow["id"],
        "slice_revision":blocked_slice["revision"],"outcome":"completed",
        "summary":"Caller reports the bounded blocker resolved and correction complete.",
        "evidence":[{"kind":"fixture_observation","reference":"native deterministic API call sequence",
                     "observation":"A later external result completed the previously blocked Slice."}],
        "scope_impact":"The bounded correction is complete.","remaining_work":"None for this Slice."})
    completed=variant(completed_payload,"created")
    terminal=ok(call,"query","slice.context",{"slice_id":follow["id"]})
    results=ok(call,"query","slice.candidates.context",{
        "scope_id":final["scope"]["id"],"view":"results","limit":25})
    result_ids={item["id"] for item in results.get("items",[])}
    terminal_attempt,terminal_failed=call("command",{"route":"slice.result.record","params":{
        "request_id":str(uuid.uuid4()),"scope_id":final["scope"]["id"],"slice_id":follow["id"],
        "slice_revision":terminal["revision"],"outcome":"completed","summary":"Duplicate completion.",
        "evidence":[{"kind":"fixture_observation","reference":"terminal guard",
                     "observation":"A second completion attempt is rejected."}],
        "scope_impact":"None.","remaining_work":"None."}})
    check("a blocked Slice accepts a later externally reported result with durable history, then becomes terminal",
          completed["result"]["outcome"]=="completed"
          and completed["result"]["slice_revision"]==blocked_slice["revision"]
          and terminal["state"]=="completed" and terminal["revision"]==blocked_slice["revision"]+1
          and {blocked["result"]["id"],completed["result"]["id"]}.issubset(result_ids)
          and terminal_failed and terminal_attempt.get("error",{}).get("code")=="forbidden",
          {"blocked_result_id":blocked["result"]["id"],
           "completed_result_id":completed["result"]["id"],"terminal_revision":terminal["revision"]})
    return {"scope_id":created["scope"]["id"],"debug_slice_id":slice_["id"],
        "result_id":result["id"],"followup_slice_id":follow["id"],"pipeline_ids":sorted(ids),
        "rule_ids":sorted(rule_ids),"result_provenance":result["provenance"],
        "claim_boundary":"Proves native API persistence, replay, freshness and branch mechanics; does not prove pipeline execution or independent verification of supplied evidence."}
