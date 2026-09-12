"""Shared native-call evidence helpers for Scope-candidate acceptance."""
from __future__ import annotations
import json, time, uuid
from typing import Any, Callable
from common import tool_result
from fixture import CANDIDATE_PLANNING_INPUT, CANDIDATE_PROGRAM_FIELDS, CANDIDATE_PROGRAM_INPUT

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

def capture_model_calls(items: list[dict[str, Any]]) -> list[dict[str, Any]]:
    """Capture complete nonsecret TectD calls without treating failure as success."""
    captured = []
    for item in items:
        if item.get("type") != "mcpToolCall":
            continue
        arguments = item.get("arguments")
        if not isinstance(arguments, dict):
            raise AssertionError("model MCP arguments are not an object")
        payload, is_error = _canonical_payload(item.get("result"))
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
    return captured
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
        "begin_arguments": {"route": "scope.candidates.begin", "params": params},
    }
def candidate_model_prompt(candidate_set_id: str) -> str:
    return (
        "Review and correct the supplied proposed breakdown for candidate set " + candidate_set_id + ". "
        "Start with get_state, then follow every exact backend-provided ready call and paging action until the "
        "complete planning context has been delivered. Read the full captured method and every matched rule "
        "before drafting. Use the exact current IDs, revisions, snapshot, source references, and route schemas "
        "returned by TectD. Apply the current TectD methodology and applicable rules, then perform and save "
        "the critical semantic review; revise and "
        "review again if needed. "
        "Use only TectD get_state, help, query, and command. Use no execute route, open no Scope, do no "
        "implementation, and do not complete the Program."
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

def assert_resolved_entities(draft: dict[str, Any]) -> None:
    """Require local draft handles to resolve to stable backend UUIDs and revisions."""
    for collection in ["goals", "evidence", "candidates", "blockers"]:
        for entity in draft.get(collection, []):
            uuid.UUID(str(entity.get("id")))
            if not isinstance(entity.get("revision"), int) or entity["revision"] < 1 or "local" in entity:
                raise AssertionError("candidate entity did not resolve to backend identity")

def assert_program_remains_open(page: dict[str, Any]) -> None:
    candidate_set = page.get("context", {}).get("candidate_set", {})
    program = page.get("program", {})
    if candidate_set.get("status") not in {"ready", "blocked"} or program.get("status") != "open":
        raise AssertionError("candidate readiness incorrectly completed or lost the Program")

def assert_review_state(context: dict[str, Any], draft: dict[str, Any], review: dict[str, Any] | None) -> None:
    """Check set status separately from per-candidate review decisions and blockers."""
    status = context.get("candidate_set", {}).get("status")
    if status == "draft":
        if review is not None:
            raise AssertionError("unreviewed draft unexpectedly has a review")
        return
    if status == "review_required":
        if not draft.get("candidates"):
            raise AssertionError("review-required set has no candidate draft")
        if review is not None and review.get("verdict") != "revise":
            raise AssertionError("review-required set has an incompatible review verdict")
        return
    if not isinstance(review, dict):
        raise AssertionError("terminal candidate status has no review")
    decisions = review.get("candidate_decisions", [])
    candidate_ids = {str(item.get("id")) for item in draft.get("candidates", [])}
    decision_ids = {str(item.get("candidate_id")) for item in decisions}
    if not candidate_ids or decision_ids != candidate_ids:
        raise AssertionError("review decisions do not cover the exact candidate set")
    if status == "ready":
        if review.get("verdict") != "ready" or any(item.get("decision") != "accept" for item in decisions):
            raise AssertionError("ready set contains a non-accepted candidate decision")
        if draft.get("blockers") or draft.get("pending_question") is not None:
            raise AssertionError("ready set retains a blocker or pending question")
    elif status == "blocked":
        material = any(item.get("severity") == "material" for item in review.get("findings", []))
        if review.get("verdict") != "blocked" or not (draft.get("blockers") or material):
            raise AssertionError("blocked set has no concrete blocker or material finding")
    else:
        raise AssertionError("unknown candidate set status")

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

def _complete_fragments(calls: list[dict[str, Any]]) -> dict[str, str]:
    bodies: dict[str, str] = {}
    complete: set[str] = set()
    for call in calls:
        params = call.get("arguments", {}).get("params", {})
        if call.get("tool") != "query" or params.get("view") != "fragment":
            continue
        fragment = call.get("payload", {}).get("fragment", {})
        source = fragment.get("source_ref", {})
        source_id = str(source.get("id"))
        text = fragment.get("text")
        cursor = fragment.get("cursor")
        if source_id != params.get("source_ref_id") or cursor != params.get("cursor") or not isinstance(text, str):
            raise AssertionError("fragment result does not match its backend-provided request")
        previous = bodies.get(source_id, "")
        if source_id in complete or cursor != len(previous.encode("utf-8")):
            raise AssertionError("fragment sequence is duplicated, skipped, or out of order")
        bodies[source_id] = previous + text
        next_cursor = fragment.get("next_cursor")
        if next_cursor is None:
            complete.add(source_id)
        elif next_cursor != len(bodies[source_id].encode("utf-8")):
            raise AssertionError("fragment continuation is not the exact server byte cursor")
    if complete != set(bodies):
        raise AssertionError("one or more referenced bodies stopped before the terminal fragment")
    return bodies

def _assert_ready_reads(reads: list[dict[str, Any]], initial: dict[str, Any] | None = None) -> None:
    def ready(payload: dict[str, Any]) -> list[tuple[str, dict[str, Any]]]:
        return [(action["tool"], action["arguments"]) for action in payload.get("actions", [])
                if action.get("kind") == "ready_call" and isinstance(action.get("arguments"), dict)]
    offered = ready(initial) if initial is not None else []
    for index, read in enumerate(reads):
        if initial is not None or index:
            try:
                offered.pop(offered.index((read["tool"], read["arguments"])))
            except ValueError as error:
                raise AssertionError("model reconstructed a context read instead of using a backend ready call") from error
        offered.extend(ready(read["payload"]))

def assert_candidate_model_result(calls: list[dict[str, Any]], scenario: dict[str, Any]) -> dict[str, Any]:
    """Validate observable workflow structure; prose remains a human semantic review."""
    assert_candidate_call_boundary(calls)
    assert_successful_calls(calls, {"get_state", "help", "query", "command"})
    draft_index = next(
        (
            index
            for index, call in enumerate(calls)
            if call["tool"] == "command"
            and call["arguments"]["route"] == "scope.candidates.save"
            and call["arguments"]["params"].get("kind") == "draft"
        ),
        None,
    )
    if draft_index is None:
        raise AssertionError("model did not save a candidate draft")
    before = calls[:draft_index]
    reads = [call for call in before if call["tool"] in {"get_state", "query"}]
    if not reads or reads[0]["tool"] != "get_state" or reads[0]["arguments"]:
        raise AssertionError("model did not start candidate recovery from get_state")
    _assert_ready_reads(reads)
    input_templates = [
        action
        for read in reads
        for action in read["payload"].get("actions", [])
        if action.get("kind") == "needs_input"
        and action.get("tool") == "command"
        and action.get("arguments", {}).get("route") == "scope.candidates.save"
    ]
    if not input_templates:
        raise AssertionError("backend context traversal did not explicitly advance to candidate drafting")
    draft_base = input_templates[-1]["arguments"]["params"]
    actual_draft_params = calls[draft_index]["arguments"]["params"]
    if {key: value for key, value in actual_draft_params.items() if key != "draft"} != draft_base:
        raise AssertionError("model did not preserve the backend-provided candidate draft call template")
    overview = next(
        call["payload"]
        for call in before
        if call["tool"] == "query" and call["arguments"]["params"].get("view") == "overview"
    )
    assert_bound_context(overview["context"], scenario["program_id"], scenario["worktree_id"])
    snapshot = overview["context"]["snapshot"]
    method = snapshot["method"]
    rules = snapshot["rules"]
    if not method["body"] or not rules or any(not rule["text"] for rule in rules):
        raise AssertionError("model context omitted the full method or matched rule bodies")
    input_items = [
        item
        for call in before
        if call["tool"] == "query" and call["arguments"]["params"].get("view") == "inputs"
        for item in call["payload"].get("items", [])
    ]
    input_sequences = [item.get("input", {}).get("sequence") for item in input_items]
    if input_sequences != [1]:
        raise AssertionError("planning input page did not preserve the exact request window")
    fragments = _complete_fragments(before)
    expected_bodies = {
        ref["id"]: scenario["planning_input"] if ref["kind"] == "planning_input" else scenario["program_fields"][ref["program_field"]]
        for ref in snapshot["source_refs"]
    }
    if fragments != expected_bodies:
        raise AssertionError("model did not retrieve every referenced Program field and planning input exactly")
    draft_call = calls[draft_index]
    stored_draft = draft_call["payload"].get("draft", {})
    context = draft_call["payload"].get("context", {})
    assert_resolved_entities(stored_draft)
    assert_review_state(context, stored_draft, None)
    planning_source_ids = {
        item["id"] for item in context["snapshot"]["source_refs"] if item["kind"] == "planning_input"
    }
    source_kinds = {item["kind"] for item in context["snapshot"]["source_refs"]}
    if source_kinds != {"program_field", "program_success", "planning_input"}:
        raise AssertionError("candidate snapshot has unexpected authority source kinds")
    goal_ids = {goal["id"] for goal in stored_draft.get("goals", [])}
    candidate_ids = {candidate["id"] for candidate in stored_draft.get("candidates", [])}
    resolved_ids = {goal.get("resolution", {}).get("id") for goal in stored_draft.get("goals", [])}
    covered_ids = {goal for candidate in stored_draft.get("candidates", []) for goal in candidate.get("coverage_goal_ids", [])}
    if stored_draft.get("boundary") != "ongoing" or stored_draft.get("evidence") or not candidate_ids or not goal_ids or any(
        goal.get("source_ref_id") not in planning_source_ids for goal in stored_draft.get("goals", [])
    ) or resolved_ids - candidate_ids or covered_ids != goal_ids:
        raise AssertionError("candidate draft is empty, claims evidence, or escapes its exact planning-input source window")
    review_calls = [
        call
        for call in calls[draft_index + 1 :]
        if call["tool"] == "command"
        and call["arguments"]["route"] == "scope.candidates.save"
        and call["arguments"]["params"].get("kind") == "review"
    ]
    if not review_calls:
        raise AssertionError("model did not save its critical candidate review")
    review_call = review_calls[-1]
    review_index = calls.index(review_call)
    post_draft_reads = [call for call in calls[draft_index + 1 : review_index] if call["tool"] == "query"]
    _assert_ready_reads(post_draft_reads, draft_call["payload"])
    review_templates = [
        action for call in post_draft_reads for action in call["payload"].get("actions", [])
        if action.get("kind") == "needs_input" and action.get("arguments", {}).get("route") == "scope.candidates.save"
    ]
    if not review_templates or {key: value for key, value in review_call["arguments"]["params"].items() if key != "review"} != review_templates[-1]["arguments"]["params"]:
        raise AssertionError("model did not preserve the backend-provided candidate review call template")
    reviewed = review_call["payload"]
    latest_review = reviewed.get("latest_review")
    if not isinstance(latest_review, dict):
        raise AssertionError("review receipt omitted the stored review")
    if reviewed.get("context", {}).get("candidate_set", {}).get("status") != "ready":
        raise AssertionError("unblocked two-outcome scenario did not reach reviewed ready state")
    if reviewed.get("recommended_action") is not None:
        raise AssertionError("terminal reviewed set still recommends a continuation loop")
    assert_review_state(reviewed.get("context", {}), reviewed.get("draft", {}), latest_review)
    return {
        "draft_arguments": draft_call["arguments"],
        "draft_payload": draft_call["payload"],
        "final_payload": reviewed,
        "method_id": method["id"],
        "method_revision": method["revision"],
        "method_digest": method["digest"],
        "rule_bindings": [{"id": rule["id"], "revision": rule["revision"], "origin_refs": rule.get("origin_refs", [])} for rule in rules],
        "registry_revision": overview["context"]["snapshot"]["registry_revision"],
        "registry_digest": overview["context"]["snapshot"]["registry_digest"],
    }

def collect_model_turn(app: Any, thread_id: str, prompt: str) -> tuple[str, list[dict[str, Any]], list[dict[str, Any]]]:
    position = len(app.notifications)
    started = app.request(
        "turn/start",
        {"threadId": thread_id, "input": [{"type": "text", "text": prompt}], "model": "gpt-5.6-sol", "effort": "medium"},
    )
    turn_id = started["turn"]["id"]
    items: list[dict[str, Any]] = []
    deadline = time.monotonic() + 600
    terminal = None
    while time.monotonic() < deadline and terminal is None:
        if position >= len(app.notifications):
            try:
                app.notifications.append(app._read(30))
            except TimeoutError:
                continue
        while position < len(app.notifications):
            event = app.notifications[position]
            position += 1
            params = event.get("params", {})
            if params.get("threadId") != thread_id:
                continue
            if event.get("method") == "item/completed" and params.get("turnId") == turn_id:
                item = params["item"]
                if item.get("type") != "reasoning":
                    items.append(item)
            if event.get("method") == "turn/completed" and params.get("turn", {}).get("id") == turn_id:
                terminal = params["turn"]
                break
    if terminal is None or terminal.get("status") != "completed":
        raise AssertionError("model turn did not complete")
    return turn_id, items, capture_model_calls(items)

def run_candidate_model_turn(app: Any, thread_id: str, scenario: dict[str, Any], proof: Any) -> None:
    turn_id, items, calls = collect_model_turn(app, thread_id, candidate_model_prompt(scenario["candidate_set_id"]))
    if {item.get("type") for item in items} - {"userMessage", "mcpToolCall", "agentMessage"}:
        raise AssertionError("candidate model used a shell, subagent, or external tool")
    evidence = assert_candidate_model_result(calls, scenario)
    replay, failed = tool_result(app, thread_id, "command", evidence["draft_arguments"])
    if failed or replay != evidence["draft_payload"]:
        raise AssertionError("exact candidate draft receipt did not replay after review")
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
        "receipt_replay": True,
    }
    proof.persist()
