"""Shared native-call evidence helpers for Scope-candidate acceptance."""
from __future__ import annotations
import json, uuid
from typing import Any, Callable
from common import tool_result
from fixture import CANDIDATE_AMENDMENT, CANDIDATE_PLANNING_INPUT, CANDIDATE_PROGRAM_FIELDS, CANDIDATE_PROGRAM_INPUT, collect_model_turn
import model_capture
import scope_candidate_cycles

CANDIDATE_QUERY_ROUTES = {"scope.candidates.context"}
CANDIDATE_COMMAND_ROUTES = {
    "scope.candidates.begin", "scope.candidates.save", "scope.candidates.record_input", "scope.candidates.refresh"
}
def _canonical_payload(result: Any) -> tuple[dict[str, Any], bool]:
    if not isinstance(result, dict):
        raise AssertionError("model MCP call has no typed result object")
    content = result.get("content")
    if not isinstance(content, list) or len(content) < 2 or content[-1].get("type") != "text":
        raise AssertionError("model MCP call lacks the canonical JSON text block")
    try:
        payload = json.loads(content[-1]["text"])
    except (KeyError, TypeError, json.JSONDecodeError) as error:
        raise AssertionError("model MCP call has invalid canonical JSON") from error
    if not isinstance(payload, dict):
        raise AssertionError("model MCP canonical payload is not an object")
    return payload, result.get("isError") is True

def capture_model_calls(items: list[dict[str, Any]]) -> tuple[list[dict[str, Any]], list[dict[str, Any]]]:
    """Parse MCP calls independently, retaining malformed call evidence."""
    captured, errors = [], []
    for item in items:
        if item.get("type") != "mcpToolCall":
            continue
        arguments = item.get("arguments")
        if not isinstance(arguments, dict):
            errors.append({"item": item, "error": "model MCP arguments are not an object"})
            continue
        try:
            payload, is_error = _canonical_payload(item.get("result"))
        except AssertionError as error:
            errors.append({"item": item, "error": str(error)})
            continue
        error_code = payload.get("error", {}).get("code") if is_error else None
        captured.append(
            {
                "server": item.get("server"),
                "tool": item.get("tool"),
                "arguments": arguments,
                "status": item.get("status"),
                "is_error": is_error,
                "error_code": error_code,
                "payload": payload,
            }
        )
    return captured, errors
def assert_successful_calls(calls: list[dict[str, Any]], allowed: set[str]) -> None:
    """Require every captured call to be an allowed successful TectD result."""
    if not calls:
        raise AssertionError("model made no MCP calls")
    for call in calls:
        if call["server"] != "tectd" or call["tool"] not in allowed:
            raise AssertionError("model used a disallowed MCP call")
        if call["status"] != "completed" or call["is_error"]:
            raise AssertionError("model MCP call did not complete successfully")
def assert_exact_recovery(
    calls: list[dict[str, Any]],
    *,
    tool: str,
    rejected_arguments: dict[str, Any],
    corrected_arguments: dict[str, Any],
    error_code: str = "invalid_arguments",
) -> None:
    """Accept one rejection only when its exact later correction is proven."""
    rejected = [
        index
        for index, call in enumerate(calls)
        if call["server"] == "tectd"
        and call["tool"] == tool
        and call["arguments"] == rejected_arguments
        and call["status"] == "failed"
        and call["is_error"]
        and call["error_code"] == error_code
    ]
    corrected = [
        index
        for index, call in enumerate(calls)
        if call["server"] == "tectd"
        and call["tool"] == tool
        and call["arguments"] == corrected_arguments
        and call["status"] == "completed"
        and not call["is_error"]
    ]
    other_failures = [
        index
        for index, call in enumerate(calls)
        if (call["status"] != "completed" or call["is_error"]) and index not in rejected
    ]
    if len(rejected) != 1 or other_failures or not any(index > rejected[0] for index in corrected):
        raise AssertionError("exact typed rejection and subsequent correction were not both proven")
def prepare_open_program(
    call: Callable[[str, dict[str, Any]], tuple[dict[str, Any], bool]],
    source_path: str,
) -> dict[str, Any]:
    """Create one open Program and select one owned source using existing live DTOs."""
    state, failed = call("get_state", {})
    if failed:
        raise AssertionError("owned fixture get_state failed")
    if state.get("status") == "uninitialized":
        state, failed = call("command", {"route": "workspace.open", "params": {}})
    if failed or not isinstance(state.get("workspace"), dict):
        raise AssertionError("owned fixture workspace did not open")
    registered, failed = call("command", {"route": "source.register", "params": {"path": source_path}})
    if failed:
        raise AssertionError("owned fixture source registration failed")
    worktree_id = registered.get("id")
    uuid.UUID(str(worktree_id))
    selected, failed = call(
        "command",
        {"route": "session.select_worktrees", "params": {"worktree_ids": [worktree_id]}},
    )
    selected_ids = [item.get("id") for item in selected.get("selected_worktrees", [])]
    if failed or selected_ids != [worktree_id]:
        raise AssertionError("owned fixture worktree selection did not persist")
    original = CANDIDATE_PROGRAM_INPUT
    begun, failed = call(
        "command",
        {"route": "program.begin", "params": {"request_id": str(uuid.uuid4()), "input": original}},
    )
    program = begun.get("program", {})
    if failed or program.get("status") != "draft":
        raise AssertionError("owned fixture Program draft was not created")
    saved, failed = call(
        "command",
        {
            "route": "program.save",
            "params": {
                "program_id": program["id"],
                "revision": program["revision"],
                "input_cursor": program["latest_input"],
                **CANDIDATE_PROGRAM_FIELDS,
                "pending_question": None,
                "complete": True,
            },
        },
    )
    opened = saved.get("program", {})
    if failed or opened.get("status") != "open" or opened.get("current_step") != "ready":
        raise AssertionError("owned fixture Program did not become open and ready")
    return {
        "program_id": opened["id"],
        "program_revision": opened["revision"],
        "worktree_id": worktree_id,
        "original_input": original,
        "program_fields": CANDIDATE_PROGRAM_FIELDS,
    }
def seed_candidate_scenario(
    call: Callable[[str, dict[str, Any]], tuple[dict[str, Any], bool]], source_path: str
) -> dict[str, Any]:
    """Begin and recover one ongoing scenario without guessing a draft payload."""
    fixture = prepare_open_program(call, source_path)
    params = {
        "request_id": str(uuid.uuid4()),
        "program_id": fixture["program_id"],
        "program_revision": fixture["program_revision"],
        "boundary": "ongoing",
        "input": CANDIDATE_PLANNING_INPUT,
    }
    begun, failed = call("command", {"route": "scope.candidates.begin", "params": params})
    context = begun.get("context", {})
    if failed or begun.get("disposition") != "created":
        raise AssertionError("owned candidate scenario was not created")
    assert_bound_context(context, fixture["program_id"], fixture["worktree_id"])
    replay, failed = call("command", {"route": "scope.candidates.begin", "params": params})
    if failed or replay.get("disposition") != "replay" or replay.get("context") != context:
        raise AssertionError("candidate begin receipt did not replay its exact context")
    candidate_set = context["candidate_set"]
    return {
        **fixture,
        "candidate_set_id": candidate_set["id"],
        "candidate_revision": candidate_set["revision"],
        "snapshot_id": context["snapshot"]["id"],
        "planning_input": CANDIDATE_PLANNING_INPUT,
        "amendment": CANDIDATE_AMENDMENT,
        "begin_arguments": {"route": "scope.candidates.begin", "params": params},
    }
def candidate_model_prompt(candidate_set_id: str) -> str:
    return (
        "Review and correct the supplied proposed breakdown for candidate set " + candidate_set_id + ". "
        "Start with get_state, then follow every exact backend-provided ready call and paging action until the "
        "complete planning context has been delivered. Read the full captured method and every matched rule "
        "before drafting. Use the exact current IDs, revisions, snapshot, source references, and route schemas "
        "returned by TectD. Apply the current TectD methodology and applicable rules, then perform and save "
        "the critical semantic review; revise and review again if needed. Only after the first planning "
        "cycle reaches Ready, record this exact separate user amendment: \"" + CANDIDATE_AMENDMENT + "\" "
        "Follow the backend record-input and refresh actions, read the complete refreshed context, and draft "
        "and critically review the amended planning result until it reaches Ready. Then read compact history, "
        "the complete original historical draft and all of its referenced source fragments, and return to the "
        "current overview. Use only TectD get_state, help, query, and command. Use no execute route, open no "
        "Scope, do no implementation, and do not complete the Program."
    )

def assert_bound_context(context: dict[str, Any], program_id: str, worktree_id: str) -> None:
    candidate_set = context.get("candidate_set", {})
    snapshot = context.get("snapshot", {})
    if candidate_set.get("program_id") != program_id:
        raise AssertionError("candidate context is not bound to the expected Program")
    if snapshot.get("selected_worktree_ids") != [worktree_id]:
        raise AssertionError("candidate context is not bound to the selected owned source")
    for key in ["method", "registry_revision", "registry_digest", "rules", "source_refs"]:
        if key not in snapshot:
            raise AssertionError(f"candidate snapshot omits {key}")
    method = snapshot["method"]
    if not all(isinstance(method.get(key), str) and method[key] for key in ["id", "revision", "digest", "body"]):
        raise AssertionError("candidate method snapshot is incomplete")
    if not snapshot["rules"] or any(not all(rule.get(key) for key in ["id", "revision", "text"]) for rule in snapshot["rules"]):
        raise AssertionError("candidate rule snapshots are incomplete")

def assert_program_remains_open(page: dict[str, Any]) -> None:
    candidate_set = page.get("context", {}).get("candidate_set", {})
    program = page.get("program", {})
    if candidate_set.get("status") not in {"ready", "blocked"} or program.get("status") != "open":
        raise AssertionError("candidate readiness incorrectly completed or lost the Program")

def assert_candidate_call_boundary(calls: list[dict[str, Any]]) -> None:
    """Keep model work inside candidate read/write routes, with no execute or Scope open."""
    for call in calls:
        tool = call.get("tool")
        arguments = call.get("arguments", {})
        if call.get("server") != "tectd" or tool not in {"get_state", "help", "query", "command"}:
            raise AssertionError("candidate model used a disallowed tool")
        if not isinstance(arguments, dict):
            raise AssertionError("candidate model call arguments are not an object")
        if tool == "get_state" and arguments:
            raise AssertionError("candidate model supplied unexpected get_state arguments")
        if tool in {"query", "command"}:
            if set(arguments) != {"route", "params"} or not isinstance(arguments["params"], dict):
                raise AssertionError("candidate routed call has the wrong envelope")
            routes = CANDIDATE_QUERY_ROUTES if tool == "query" else CANDIDATE_COMMAND_ROUTES
            if arguments["route"] not in routes:
                raise AssertionError(f"candidate model used a disallowed {tool} route")
        if tool == "help" and "route" in arguments:
            if arguments["route"] not in CANDIDATE_QUERY_ROUTES | CANDIDATE_COMMAND_ROUTES:
                raise AssertionError("candidate model requested help for a disallowed route")

def assert_model_item_boundary(items: list[dict[str, Any]]) -> None:
    # These non-action item variants are defined by the generated local app-server ThreadItem schema.
    passive = {"userMessage", "agentMessage", "plan", "reasoning", "contextCompaction"}
    unexpected = sorted({str(item.get("type")) for item in items} - passive - {"mcpToolCall"})
    if unexpected:
        raise AssertionError("candidate model used disallowed item types: " + ", ".join(unexpected))

def assert_candidate_model_result(calls: list[dict[str, Any]], scenario: dict[str, Any]) -> dict[str, Any]:
    """Validate two observable cycles; prose remains a human semantic review."""
    assert_candidate_call_boundary(calls)
    assert_successful_calls(calls, {"get_state", "help", "query", "command"})
    evidence = scope_candidate_cycles.assert_two_cycles(calls, scenario)
    first = evidence["first_snapshot"]
    second = evidence["second_snapshot"]
    for snapshot in [first, second]:
        method, rules = snapshot["method"], snapshot["rules"]
        if not method["body"] or not rules or any(not rule["text"] for rule in rules):
            raise AssertionError("planning context omitted the full method or matched rule bodies")
    evidence["method_id"] = second["method"]["id"]
    evidence["method_revision"] = second["method"]["revision"]
    evidence["method_digest"] = second["method"]["digest"]
    evidence["rule_bindings"] = [
        {"id": rule["id"], "revision": rule["revision"], "origin_refs": rule.get("origin_refs", [])}
        for rule in second["rules"]
    ]
    evidence["registry_revision"] = second["registry_revision"]
    evidence["registry_digest"] = second["registry_digest"]
    return evidence

def run_candidate_model_turn(app: Any, thread_id: str, scenario: dict[str, Any], proof: Any, allow_one_child: bool = False) -> None:
    turn_id, items = collect_model_turn(
        app, thread_id, candidate_model_prompt(scenario["candidate_set_id"]), proof, allow_one_child,
    )
    calls, parse_errors = capture_model_calls(items)
    proof.data["scope_candidate_model_capture"]["calls"] = calls
    proof.data["scope_candidate_model_capture"]["parse_errors"] = parse_errors
    proof.persist()
    if parse_errors:
        raise AssertionError("candidate model emitted malformed MCP call evidence")
    model_capture.assert_child_item_boundary(items) if allow_one_child else assert_model_item_boundary(items)
    evidence = assert_candidate_model_result(calls, scenario)
    for arguments, payload in evidence["draft_receipts"]:
        replay, failed = tool_result(app, thread_id, "command", arguments)
        if failed or replay != payload:
            raise AssertionError("exact candidate draft receipt did not replay after later planning")
    current, failed = tool_result(
        app,
        thread_id,
        "query",
        {"route": "scope.candidates.context", "params": {"candidate_set_id": scenario["candidate_set_id"], "view": "program", "limit": 25}},
    )
    if failed:
        raise AssertionError("candidate recovery query failed")
    assert_program_remains_open(current)
    if current["context"]["candidate_set"]["revision"] != evidence["final_payload"]["context"]["candidate_set"]["revision"]:
        raise AssertionError("candidate recovery did not retain the current reviewed revision")
    proof.data["scope_candidate_model_turn"] = {
        "thread_id": thread_id,
        "turn_id": turn_id,
        "model": "gpt-5.6-sol",
        "effort": "medium",
        "calls": calls,
        "semantic_review_required": True,
        "bindings": {
            key: evidence[key]
            for key in ["method_id", "method_revision", "method_digest", "rule_bindings", "registry_revision", "registry_digest"]
        },
        "receipt_replays": len(evidence["draft_receipts"]),
    }
    proof.persist()
