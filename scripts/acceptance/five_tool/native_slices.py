"""Deterministic native Scope/Slice flow inside the existing five-tool acceptance."""
from __future__ import annotations
import json, uuid
from typing import Any, Callable
import pipeline_execution
import scope_candidates

SLICE_RUN_PIPELINES = {
    "slice.lightweight-tdd-development", "slice.full-design-to-execution",
    "slice.debug-root-cause", "slice.operational-preparation", "slice.operational-execution",
    "slice.research-to-durable-knowledge", "slice.custom-procedure-capture",
}
PROMOTION_PIPELINE = "slice.promote-to-durable-knowledge"
ALL_PIPELINES = SLICE_RUN_PIPELINES | {PROMOTION_PIPELINE}
PIPELINE_MODES = {
    "slice.lightweight-tdd-development": ("whole", ["whole", "phasewise"], 14),
    "slice.full-design-to-execution": ("phasewise", ["phasewise"], 20),
    "slice.debug-root-cause": ("whole", ["whole", "phasewise"], 18),
    "slice.operational-preparation": ("whole", ["whole", "phasewise"], 16),
    "slice.operational-execution": ("phasewise", ["phasewise"], 18),
    "slice.research-to-durable-knowledge": ("phasewise", ["whole", "phasewise"], 22),
    "slice.custom-procedure-capture": ("whole", ["whole", "phasewise"], 17),
}
PIPELINE_DEFINITIONS = {
    "slice.lightweight-tdd-development": ("0.4.0-native.skills.1", "b80b3472ebf4acc38996fa1946a2fe76e1b17fbcc39c6594f87a00e63a437768"),
    "slice.full-design-to-execution": ("0.4.0-native.skills.1", "13fd152337abc76d7bbfa15c0875d7b6fbe4719ccfd6fadab5f31825cd39769b"),
    "slice.debug-root-cause": ("0.4.0-native.skills.1", "ecd89aaae1265455b79b400f200a7f932a596dbd0c06700a90b0116f7aaeb2ac"),
    "slice.operational-preparation": ("0.4.0-native.skills.1", "db83e347ee7970d2122dc999ec6cefd8e2e88ac9e6a944e3be3fc1554cbc414a"),
    "slice.operational-execution": ("0.4.0-native.skills.1", "8a1be05166244cffae5456f476d9748051f4786f3c9c2f4744b1abace4facdb9"),
    "slice.research-to-durable-knowledge": ("0.4.0-native.skills.1", "374987b7516fe57c4de4282ace1a0fd80712e0bcac664ee08057ab340fc0b8ce"),
    "slice.custom-procedure-capture": ("0.4.0-native.skills.1", "b8bb5affd153f1642f120625f71fd6f0b0a1b5dd877f2e46cc0cb589f8153b8c"),
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

def assert_exact_pipeline_delivery(context: dict[str, Any], kind: str, check: Callable) -> None:
    default, allowed, phase_count = PIPELINE_MODES[kind]
    version, digest = PIPELINE_DEFINITIONS[kind]
    mode = context["run"]["delivery_mode"]
    phases = context["definition"].get("phases", [])
    delivered = context.get("delivered_phases", [])
    items = [item for phase in phases for field in ("instructions", "skills", "resources")
             for item in phase.get(field, [])]
    expected = phase_count if mode == "whole" else 1
    check(f"{kind} delivers exact pinned bodies through the native Codex MCP surface",
          context["definition"].get("kind") == kind
          and context["definition"].get("version") == version
          and context["definition"].get("digest") == digest
          and context["run"].get("definition_version") == version
          and context["run"].get("definition_digest") == digest
          and len(phases) == expected
          and len(delivered) == expected and bool(items)
          and all(isinstance(item.get("id"),str) and item["id"].strip()
                  and isinstance(item.get("version"),str) and item["version"].strip()
                  and isinstance(item.get("digest"),str) and item["digest"].strip()
                  and isinstance(item.get("body"),str) and item["body"].strip()
                  and isinstance(item.get("origin_refs"),list) and item["origin_refs"]
                  for item in items),
          {"kind":kind,"mode":mode,"default":default,"allowed":allowed,
           "version":context["definition"].get("version"),
           "digest":context["definition"].get("digest"),
           "delivered_phases":len(delivered),"delivered_items":len(items)})

def run(call: Callable, source_path: str, check: Callable) -> dict[str, Any]:
    source = source_candidate(call, source_path)
    catalogue = ok(call, "query", "slice.pipelines", {})
    entries = catalogue.get("pipelines", []); ids = {entry.get("kind") for entry in entries}
    by_kind = {entry.get("kind"):entry for entry in entries}
    promotion = by_kind.get(PROMOTION_PIPELINE, {})
    slice_run_ids = {kind for kind, entry in by_kind.items()
                     if entry.get("execution_owner", "slice_pipeline_run") == "slice_pipeline_run"}
    check("native catalogue separates eight kinds from seven ordinary SlicePipelineRun kinds",
          ids == ALL_PIPELINES and slice_run_ids == SLICE_RUN_PIPELINES
          and "slice.hybrid-implementation-operation" not in json.dumps(catalogue)
          and catalogue.get("revision") == "3"
          and catalogue.get("executable") is True and catalogue.get("executable_count") == 8
          and all(by_kind[kind].get("implementation_status") == "executable"
                  and by_kind[kind].get("description_status") == "refined"
                  and by_kind[kind].get("refinement_required") is False
                  and by_kind[kind].get("default_delivery_mode") == values[0]
                  and by_kind[kind].get("allowed_delivery_modes") == values[1]
                  for kind,values in PIPELINE_MODES.items())
          and promotion.get("execution_owner") == "knowledge_change"
          and promotion.get("default_delivery_mode") == "whole"
          and promotion.get("allowed_delivery_modes") == ["whole", "phasewise"],
          {"pipeline_ids":sorted(ids),"slice_run_pipeline_ids":sorted(slice_run_ids),
           "executable_count":catalogue.get("executable_count"),
           "promotion_owner":promotion.get("execution_owner")})

    entry = catalogue.get("knowledge_change_entry", {})
    definition = entry.get("definition", {})
    overview = definition.get("overview", {})
    phases = definition.get("phases", [])
    phase_ids = [phase.get("id") for phase in phases]
    phase_methods = {method.get("id") for phase in phases
                     for method in phase.get("methods", [])
                     if str(method.get("id", "")).startswith("tect:knowledge-change:kc-")}
    profile_methods = {method.get("id") for phase in phases
                       for method in phase.get("methods", [])
                       if str(method.get("id", "")).startswith("tect:knowledge-profile:")}
    expected_phases = [
        "kc-intake", "kc-resolve-baseline", "kc-qualify-plan", "kc-qualify-evidence",
        "kc-prepare-change", "kc-domain-checks", "kc-impact-plan", "kc-review-reconcile",
        "kc-publication-gate", "kc-commit", "kc-settle-effects", "kc-result-handoff",
    ]
    expected_profiles = {
        "tect:knowledge-profile:general", "tect:knowledge-profile:runbook",
        "tect:knowledge-profile:protocol", "tect:knowledge-profile:devops",
        "tect:knowledge-profile:operations", "tect:knowledge-profile:product_research",
        "tect:knowledge-profile:security",
    }
    promotion_declares_unconfigured_vectors = (
        "vectors that are not configured" in catalogue.get("promotion_method", {}).get("body", "")
    )
    check("catalogue exposes the static twelve-phase Knowledge Change and seven profile methods",
          entry.get("route") == "knowledge.change_begin"
          and entry.get("context_route") == "knowledge.lifecycle"
          and definition.get("default_mode") == "whole"
          and definition.get("allowed_modes") == ["whole", "phasewise"]
          and definition.get("version") == "0.4.0-dk4.1"
          and definition.get("registry_version") == "0.2.0-dk2.1"
          and overview.get("version") == "0.4.0-dk4.1"
          and overview.get("origin_refs") == ["crates/host/knowledge-methods/overview-dk4.md"]
          and "With optional vector capability enabled" in overview.get("body", "")
          and "Vector/search work is not configured" not in overview.get("body", "")
          and "Required unavailable capabilities block" not in overview.get("body", "")
          and definition.get("completion_contract_ref", {}).get("version") == "0.4.0-dk4.1"
          and definition.get("escalation_contract_ref", {}).get("version") == "0.4.0-dk4.1"
          and phase_ids == expected_phases and len(phase_methods) == 9
          and profile_methods == expected_profiles
          and catalogue.get("phase_counts", {}).get("knowledge_change_phases") == 12
          and promotion_declares_unconfigured_vectors,
          {"phase_ids":phase_ids,"phase_method_ids":sorted(phase_methods),
           "profile_method_ids":sorted(profile_methods),
           "knowledge_change_version":definition.get("version"),
           "knowledge_change_overview_version":overview.get("version"),
           "knowledge_change_overview_digest":overview.get("digest"),
           "promotion_method_declares_unconfigured_vectors":promotion_declares_unconfigured_vectors})

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
    check("slice.open starts no non-executable Debug run and reinjects no design rules",
          slice_["pipeline"]=="slice.debug-root-cause" and slice_["pipeline_status"]=="not_started"
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

    probe_kinds = [kind for kind in PIPELINE_MODES if kind != "slice.lightweight-tdd-development"]
    probe_nodes = [{"kind":"work","identity":{"local":f"probe-{index}"},
        "title":f"Inspect native delivery for {kind}",
        "outcome":"The pinned definition and current exact step bodies are delivered through MCP.",
        "includes":["native delivery inspection"],"excludes":["semantic execution"],
        "dependencies":[{"candidate_id":debug["id"],"revision":debug["revision"]}],
        "proof":["Exact version, digest, body and origin reference are present."],
        "pipeline":kind,"pipeline_reason":"Bounded final native delivery acceptance.",
        "source_result_ids":[result["id"]]} for index,kind in enumerate(probe_kinds,1)]
    for node in probe_nodes:
        if node["pipeline"] == "slice.full-design-to-execution":
            node["why_lightweight_insufficient"] = (
                "Full delivery is the object under inspection, including its irreducible phase contract."
            )
            node["why_further_vertical_split_not_viable"] = (
                "Splitting the inspection would no longer prove one pinned Full definition delivery."
            )
    revised=save_plan(call,refreshed,{"coverage_summary":"Result resolves decision and enables exact pipeline delivery inspection.","nodes":[
        existing_work(debug),
        {"kind":"work","identity":{"local":"correction"},"title":"Correct preview derivation",
         "outcome":"Preview reflects saved recipient and cadence settings.",
         "includes":["bounded correction","regression proof"],"excludes":["redesign","deployment"],
         "dependencies":[{"candidate_id":debug["id"],"revision":debug["revision"]}],
         "proof":["Focused regression and existing preview tests pass."],
         "pipeline":"slice.lightweight-tdd-development",
         "pipeline_reason":"The result isolates one small vertical correction.",
         "source_result_ids":[result["id"]]},
    ] + probe_nodes,"supersessions":[{"candidate_id":decision["id"],"revision":decision["revision"],
        "reason":"Diagnosis resolves the correction-path question.","replacements":[{"local":"correction"}]}]})
    light=next(n for n in revised["draft"]["nodes"] if n["pipeline"]=="slice.lightweight-tdd-development")
    final=review_plan(call,revised,"Result-backed Lightweight successor is bounded and history explicit.")
    follow=variant(ok(call,"command","slice.open",open_params(final,light)),"created")
    check("externally reported result supports Decision-to-Lightweight continuation",
          follow["pipeline"]=="slice.lightweight-tdd-development" and follow["execution_claimed"] is False,
          {"slice_id":follow["id"],"source_result_id":result["id"]})
    probe_runs = {}
    for kind in probe_kinds:
        candidate = next(node for node in final["draft"]["nodes"] if node.get("pipeline") == kind
                         and node.get("id") != debug["id"])
        probe = variant(ok(call,"command","slice.open",open_params(final,candidate)),"created")
        explicit_mode = "whole" if kind == "slice.research-to-durable-knowledge" else None
        begun_probe = ok(call,"command","slice.pipeline.begin",pipeline_execution.begin_params(
            final["scope"]["id"],probe["id"],probe["revision"],delivery_mode=explicit_mode,
            qualification_reason="Bounded native delivery inspection; no semantic execution claim."))
        probe_context = pipeline_execution.outcome(begun_probe,"created")
        assert_exact_pipeline_delivery(probe_context,kind,check)
        probe_runs[kind] = probe_context["run"]["id"]
    begun=ok(call,"command","slice.pipeline.begin",pipeline_execution.begin_params(
        final["scope"]["id"],follow["id"],follow["revision"]))
    context=pipeline_execution.outcome(begun,"created")
    pipeline_execution.assert_lightweight_whole_context(context,check)
    assert_exact_pipeline_delivery(context,"slice.lightweight-tdd-development",check)
    bypass, bypass_failed=call("command",{"route":"slice.result.record","params":{
        "request_id":str(uuid.uuid4()),"scope_id":final["scope"]["id"],"slice_id":follow["id"],
        "slice_revision":follow["revision"],"outcome":"blocked","summary":"Managed bypass attempt.",
        "evidence":[{"kind":"fixture_observation","reference":"managed guard",
                     "observation":"Legacy result recording is rejected for a managed run."}],
        "scope_impact":"None.","remaining_work":"Use the managed run."}})
    check("managed Lightweight run rejects legacy Result bypass",
          bypass_failed and bypass.get("error",{}).get("code")=="forbidden",
          {"error_code":bypass.get("error",{}).get("code")})

    first=ok(call,"command","slice.pipeline.phase.complete",
             pipeline_execution.completion_params(context))
    context=first["context"]
    escalated=ok(call,"command","slice.pipeline.delivery.escalate",{
        "request_id":str(uuid.uuid4()),"run_id":context["run"]["id"],
        "run_revision":context["run"]["revision"],"phase_id":context["run"]["current_phase_id"],
        "reason":"The remaining isolated acceptance is inspected one phase at a time."})
    context=escalated["context"]
    check("whole-to-phasewise escalation resumes at the first unfinished phase",
          context["run"]["delivery_mode"]=="phasewise"
          and context["run"]["current_phase_ordinal"]==2
          and len(context["delivered_phases"])==1,
          {"current_phase":context["run"]["current_phase_id"]})

    waiting=ok(call,"command","slice.pipeline.phase.complete",
               pipeline_execution.completion_params(context,outcome_name="waiting_input"))
    context=waiting["context"]
    supplied=ok(call,"command","slice.pipeline.input",{
        "request_id":str(uuid.uuid4()),"run_id":context["run"]["id"],
        "run_revision":context["run"]["revision"],"phase_id":context["run"]["current_phase_id"],
        "input":"Owned fixture supplies bounded resume context without changing authority."})
    context=supplied["context"]
    resumed=ok(call,"command","slice.pipeline.phase.complete",
               pipeline_execution.completion_params(context))
    context=resumed["context"]
    while context["run"]["current_phase_ordinal"] < 14:
        context=ok(call,"command","slice.pipeline.phase.complete",
                   pipeline_execution.completion_params(context))["context"]

    terminal_result={
        "summary":"Caller reports the final Lightweight phase temporarily blocked.",
        "evidence":[{"kind":"fixture_observation","reference":"native deterministic API call sequence",
                     "observation":"A managed blocked Result was explicitly requested."}],
        "scope_impact":"Future planning receives the reported blocker.",
        "remaining_work":"Supply resume evidence and finish the final phase.",
    }
    blocked_payload=ok(call,"command","slice.pipeline.phase.complete",
        pipeline_execution.completion_params(context,outcome_name="blocked",transition="block",
            terminal_result=terminal_result,publish_blocked_result=True))
    blocked=blocked_payload["result"]; context=blocked_payload["context"]
    context=ok(call,"command","slice.pipeline.input",{
        "request_id":str(uuid.uuid4()),"run_id":context["run"]["id"],
        "run_revision":context["run"]["revision"],"phase_id":context["run"]["current_phase_id"],
        "input":"Owned fixture reports that the final blocker is resolved."})["context"]
    terminal_result["summary"]="Caller reports the managed Lightweight Slice complete."
    terminal_result["remaining_work"]="None for this Slice."
    completed_payload=ok(call,"command","slice.pipeline.phase.complete",
        pipeline_execution.completion_params(context,transition="complete",terminal_result=terminal_result))
    completed=completed_payload["result"]
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
          completed["outcome"]=="completed" and completed["provenance"]=="externally_reported"
          and completed["pipeline_run_id"]==context["run"]["id"]
          and terminal["state"]=="completed"
          and {blocked["id"],completed["id"]}.issubset(result_ids)
          and terminal_failed and terminal_attempt.get("error",{}).get("code")=="forbidden",
          {"blocked_result_id":blocked["id"],
           "completed_result_id":completed["id"],"terminal_revision":terminal["revision"]})
    return {"scope_id":created["scope"]["id"],"debug_slice_id":slice_["id"],
        "result_id":result["id"],"followup_slice_id":follow["id"],"pipeline_ids":sorted(ids),
        "slice_run_pipeline_ids":sorted(slice_run_ids),
        "rule_ids":sorted(rule_ids),"result_provenance":result["provenance"],
        "pipeline_probe_run_ids":probe_runs,
        "claim_boundary":"Proves native API persistence, exact pipeline delivery, durable managed transitions and structural receipt enforcement; caller-supplied fixture evidence does not prove independent semantic verification."}
