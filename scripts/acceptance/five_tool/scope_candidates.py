"""Shared native-call evidence helpers for Scope-candidate acceptance."""
from __future__ import annotations
import json, uuid
from typing import Any, Callable
from common import tool_result
from fixture import CANDIDATE_AMENDMENT, CANDIDATE_PLANNING_INPUT, CANDIDATE_PROGRAM_FIELDS, CANDIDATE_PROGRAM_INPUT, collect_model_turn
import model_capture
import scope_candidate_cycles

CANDIDATE_QUERY_ROUTES = {"program.get", "source.list", "scope.candidates.context"}
CANDIDATE_COMMAND_ROUTES = {
    "workspace.open", "session.select_worktrees", "scope.candidates.begin", "scope.candidates.save",
    "scope.candidates.record_input", "scope.candidates.refresh",
}
def _canonical_payload(result: Any) -> tuple[dict[str, Any], bool]:
    if not isinstance(result, dict):
        raise AssertionError("model MCP call has no typed result object")
    content = result.get("content")
    if not isinstance(content, list) or len(content) < 2 or content[1].get("type") != "text":
        raise AssertionError("model MCP call lacks the canonical JSON text block")
    try:
        payload = json.loads(content[1]["text"])
    except (KeyError, TypeError, json.JSONDecodeError) as error:
        raise AssertionError("model MCP call has invalid canonical JSON") from error
    if not isinstance(payload, dict):
        raise AssertionError("model MCP canonical payload is not an object")
    return payload, result.get("isError") is True

def _wire_call(pair: dict[str, Any]) -> dict[str, Any]:
    request, response = pair.get("request"), pair.get("response")
    if not isinstance(request, dict) or not isinstance(response, dict):
        raise AssertionError("MCP wire pair is not a request and response object")
    params = request.get("params")
    if not isinstance(params, dict) or not isinstance(params.get("arguments", {}), dict):
        raise AssertionError("MCP wire request has invalid tool parameters")
    metadata = params.get("_meta") if isinstance(params.get("_meta"), dict) else {}
    actor = metadata.get("threadId")
    if pair.get("response_source") == "fixture_capture":
        error = response.get("error", {})
        return {
            "source": "fixture_capture", "server": "tectd", "tool": params.get("name"),
            "arguments": params.get("arguments", {}), "actor_thread_id": actor,
            "response_source": pair.get("response_source"), "forwarded": pair.get("forwarded"),
            "status": "failed", "is_error": True, "error_code": error.get("message"),
            "payload": {}, "wire_request": request, "wire_response": response,
            "connection_id": pair.get("connection_id"),
        }
    if "error" in response:
        error = response.get("error", {})
        return {
            "source": "mcp_wire", "server": "tectd", "tool": params.get("name"),
            "arguments": params.get("arguments", {}), "actor_thread_id": actor,
            "response_source": pair.get("response_source"), "forwarded": pair.get("forwarded"),
            "status": "failed", "is_error": True, "error_code": error.get("code"),
            "payload": {}, "wire_request": request, "wire_response": response,
            "connection_id": pair.get("connection_id"),
        }
    payload, is_error = _canonical_payload(response.get("result"))
    return {
        "source": "mcp_wire", "server": "tectd", "tool": params.get("name"),
        "arguments": params.get("arguments", {}), "actor_thread_id": actor,
        "response_source": pair.get("response_source"), "forwarded": pair.get("forwarded"),
        "status": "failed" if is_error else "completed", "is_error": is_error,
        "error_code": payload.get("error", {}).get("code") if is_error else None,
        "payload": payload, "wire_request": request, "wire_response": response,
        "connection_id": pair.get("connection_id"),
    }


def capture_model_calls(items: list[dict[str, Any]]) -> tuple[list[dict[str, Any]], list[dict[str, Any]]]:
    """Parse MCP calls independently, retaining malformed call evidence."""
    captured, errors = [], []
    for item in items:
        if item.get("source") == "mcp_wire":
            try:
                captured.append(_wire_call(item))
            except AssertionError as error:
                errors.append({"pair": item, "error": str(error)})
            continue
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
def assert_successful_calls(
    calls: list[dict[str, Any]], allowed: set[str], recovered_failures: set[int] | None = None,
) -> None:
    """Require every call except an exactly proven recovery to be a backend success."""
    if not calls:
        raise AssertionError("model made no MCP calls")
    recovered = recovered_failures or set()
    for index, call in enumerate(calls):
        if call.get("source") != "mcp_wire":
            raise AssertionError("model MCP call did not reach the TectD backend")
        if call["server"] != "tectd" or call["tool"] not in allowed:
            raise AssertionError("model used a disallowed MCP call")
        if index in recovered:
            if call["status"] == "completed" or not call["is_error"]:
                raise AssertionError("declared recovered MCP call is not a rejection")
            continue
        if call["status"] != "completed" or call["is_error"]:
            raise AssertionError("model MCP call did not complete successfully")
def recovered_backend_failures(calls: list[dict[str, Any]]) -> set[int]:
    """Accept a typed backend rejection only after its exact ready recovery call."""
    def backend(call: dict[str, Any]) -> bool:
        response_source = call.get("response_source")
        return (
            call.get("source") == "mcp_wire"
            and call.get("forwarded") is True
            and (
                response_source == "mcp_wire"
                or (response_source is None and call.get("origin") == "tectd")
            )
        )

    recovered: set[int] = set()
    for index, call in enumerate(calls):
        if call.get("status") == "completed" and not call.get("is_error"):
            continue
        if (
            not backend(call)
            or call.get("server") != "tectd"
            or not isinstance(call.get("error_code"), str)
            or not call["error_code"]
        ):
            raise AssertionError("failed MCP call is not an authoritative typed backend rejection")
        actions = [
            (action.get("tool"), action.get("arguments"))
            for action in call.get("payload", {}).get("actions", [])
            if action.get("kind") == "ready_call"
            and isinstance(action.get("tool"), str)
            and isinstance(action.get("arguments"), dict)
        ]
        if index + 1 >= len(calls) or not actions:
            raise AssertionError("backend rejection was not followed by an exact recovery action")
        following = calls[index + 1]
        if (
            (following.get("tool"), following.get("arguments")) not in actions
            or not backend(following)
            or following.get("status") != "completed"
            or following.get("is_error")
        ):
            raise AssertionError("backend rejection was not followed by an exact recovery action")
        recovered.add(index)
    return recovered
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
def candidate_model_prompt(candidate_set_id: str, worktree_id: str) -> str:
    return (
        "Review and correct the supplied proposed breakdown for candidate set " + candidate_set_id + ". "
        "Start with get_state and its workspace.open action. This new native session must use the existing owned "
        "fixture worktree " + worktree_id + ". After workspace.open and before reading candidate context, call "
        "query source.list with limit 25, verify that exact worktree ID is present, then call command "
        "session.select_worktrees with exactly that one ID. Do not register or select another source. Then follow "
        "every exact backend-provided ready call and paging action until the "
        "complete planning context has been delivered. Read the full captured method and every matched rule "
        "before drafting. Use the exact current IDs, revisions, snapshot, source references, and route schemas "
        "returned by TectD. Never create a request ID or other control value: fill only the authored input fields "
        "declared by an exact backend action. If a call is rejected, execute its exact backend recovery action "
        "before any other discovery or mutation, then correct only the declared authored field under the unchanged "
        "control envelope. Apply the current TectD methodology and applicable rules, then perform and save "
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
    recovered = recovered_backend_failures(calls)
    assert_successful_calls(calls, {"get_state", "help", "query", "command"}, recovered)
    opened = [call for call in calls if call["tool"] == "command"
              and call["arguments"].get("route") == "workspace.open"]
    if len(opened) != 1 or opened[0]["payload"].get("session", {}).get("native_session_id") != opened[0].get("actor_thread_id"):
        raise AssertionError("child workspace bootstrap does not bind its native MCP identity")
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

def run_candidate_model_turn(
    app: Any, thread_id: str, scenario: dict[str, Any], proof: Any,
    allow_one_child: bool = False, capture_fixture: Any = None,
) -> None:
    turn_id, items = collect_model_turn(
        app, thread_id, candidate_model_prompt(scenario["candidate_set_id"], scenario["worktree_id"]), proof,
        allow_one_child, capture_fixture,
    )
    calls, parse_errors = capture_model_calls(items)
    proof.data["scope_candidate_model_capture"]["calls"] = calls
    proof.data["scope_candidate_model_capture"]["parse_errors"] = parse_errors
    proof.persist()
    if parse_errors:
        raise AssertionError("candidate model emitted malformed MCP call evidence")
    if not allow_one_child:
        assert_model_item_boundary(items)
    else:
        if any(call.get("source") != "mcp_wire" for call in calls):
            raise AssertionError("one-child call oracle did not use the MCP wire")
        capture_fixture.disarm_model_capture()
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
