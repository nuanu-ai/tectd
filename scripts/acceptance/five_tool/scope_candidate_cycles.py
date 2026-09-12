"""Assertions for two sequential native Scope-candidate planning cycles."""
from __future__ import annotations

import uuid
from typing import Any


def _is(call: dict[str, Any], tool: str, route: str | None = None) -> bool:
    if call.get("tool") != tool:
        return False
    return route is None or call.get("arguments", {}).get("route") == route


def _save_kind(call: dict[str, Any], kind: str) -> bool:
    return _is(call, "command", "scope.candidates.save") and call["arguments"]["params"].get("kind") == kind


def _ready(payload: dict[str, Any]) -> list[tuple[str, dict[str, Any]]]:
    return [
        (action["tool"], action["arguments"])
        for action in payload.get("actions", [])
        if action.get("kind") in {"ready_call", "needs_input"}
        and isinstance(action.get("tool"), str)
        and isinstance(action.get("arguments"), dict)
    ]


def _assert_offered_reads(
    reads: list[dict[str, Any]], initial: dict[str, Any] | None = None,
    explicit: list[tuple[str, dict[str, Any]]] | None = None,
) -> None:
    def calls(payload: dict[str, Any]) -> list[tuple[str, dict[str, Any]]]:
        return [
            (action["tool"], action["arguments"])
            for action in payload.get("actions", [])
            if action.get("kind") == "ready_call"
            and isinstance(action.get("tool"), str)
            and isinstance(action.get("arguments"), dict)
        ]

    offered = calls(initial) if initial is not None else []
    supplied = list(explicit or [])
    for index, read in enumerate(reads):
        call = (read["tool"], read["arguments"])
        supplied_call = False
        if call in supplied:
            supplied.remove(call)
            supplied_call = True
        if read["tool"] != "help" and not supplied_call and (initial is not None or index):
            try:
                offered.pop(offered.index(call))
            except ValueError as error:
                raise AssertionError("model reconstructed a context read instead of using a backend action") from error
        offered.extend(calls(read["payload"]))
    if supplied:
        raise AssertionError("model omitted an explicitly supplied fixture setup call")


def _assert_source_setup(
    calls: list[dict[str, Any]], before: int, scenario: dict[str, Any],
) -> list[tuple[str, dict[str, Any]]]:
    prefix = calls[:before]
    opened = [index for index, call in enumerate(prefix) if _is(call, "command", "workspace.open")]
    listed = [index for index, call in enumerate(calls) if _is(call, "query", "source.list")]
    selected = [index for index, call in enumerate(calls) if _is(call, "command", "session.select_worktrees")]
    contexts = [index for index, call in enumerate(prefix) if _is(call, "query", "scope.candidates.context")]
    refreshes = [index for index, call in enumerate(prefix) if _is(call, "command", "scope.candidates.refresh")]
    if len(opened) != 1 or len(listed) != 1 or len(selected) != 1 or not contexts:
        raise AssertionError("new child session did not perform exactly one owned-source setup")
    if not opened[0] < listed[0] < selected[0] < min(contexts + refreshes):
        raise AssertionError("owned-source setup did not finish before candidate context traversal")
    expected_list = {"route": "source.list", "params": {"limit": 25}}
    expected_select = {
        "route": "session.select_worktrees",
        "params": {"worktree_ids": [scenario["worktree_id"]]},
    }
    list_call, select_call = calls[listed[0]], calls[selected[0]]
    if list_call["arguments"] != expected_list or select_call["arguments"] != expected_select:
        raise AssertionError("owned-source setup changed its exact supplied arguments")
    actor = prefix[opened[0]].get("actor_thread_id")
    if not actor or list_call.get("actor_thread_id") != actor or select_call.get("actor_thread_id") != actor:
        raise AssertionError("owned-source setup did not use the opened child session")
    listed_ids = [item.get("id") for item in list_call["payload"].get("items", [])]
    if listed_ids != [scenario["worktree_id"]] or list_call["payload"].get("next_after") is not None:
        raise AssertionError("owned source catalog did not contain exactly the seeded fixture worktree")
    selected_ids = [item.get("id") for item in select_call["payload"].get("selected_worktrees", [])]
    if selected_ids != [scenario["worktree_id"]]:
        raise AssertionError("new child session did not retain the exact owned worktree selection")
    return [("query", expected_list), ("command", expected_select)]


def _assert_source_snapshots(calls: list[dict[str, Any]], scenario: dict[str, Any]) -> None:
    current, historical = [], []
    for call in calls:
        payload = call.get("payload", {})
        context = payload.get("context", {})
        if isinstance(context.get("snapshot"), dict):
            current.append(context["snapshot"])
        old = payload.get("historical", {})
        if isinstance(old, dict) and isinstance(old.get("snapshot"), dict):
            historical.append(old["snapshot"])
    if not current or not historical:
        raise AssertionError("candidate proof omitted current or historical snapshots")
    if any(snapshot.get("selected_worktree_ids") != [scenario["worktree_id"]]
           for snapshot in current + historical):
        raise AssertionError("candidate planning switched away from the selected owned source")


def _assert_command_template(
    calls: list[dict[str, Any]], index: int, payloads: list[dict[str, Any]], variable: str | None,
) -> None:
    actual = calls[index]
    candidates = [action for payload in payloads for action in _ready(payload)
                  if action[0] == actual["tool"]]
    if variable is None:
        expected = actual["arguments"]
        matched = expected in [arguments for _, arguments in candidates]
    else:
        params = actual["arguments"].get("params", {})
        base = {key: value for key, value in params.items() if key != variable}
        matched = any(
            arguments.get("route") == actual["arguments"].get("route")
            and {key: value for key, value in arguments.get("params", {}).items() if key != variable} == base
            for _, arguments in candidates
        )
    if not matched:
        raise AssertionError("model did not preserve the backend-provided command template")


def _fragments(calls: list[dict[str, Any]], draft_revision: int | None) -> dict[str, str]:
    bodies: dict[str, str] = {}
    complete: set[str] = set()
    for call in calls:
        params = call.get("arguments", {}).get("params", {})
        if not _is(call, "query", "scope.candidates.context") or params.get("view") != "fragment":
            continue
        if params.get("draft_revision") != draft_revision:
            continue
        fragment = call.get("payload", {}).get("fragment", {})
        source = fragment.get("source_ref", {})
        source_id = str(source.get("id"))
        text, cursor = fragment.get("text"), fragment.get("cursor")
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


def _expected_bodies(snapshot: dict[str, Any], scenario: dict[str, Any], inputs: dict[int, str]) -> dict[str, str]:
    expected = {}
    for source in snapshot["source_refs"]:
        if source["kind"] == "planning_input":
            expected[source["id"]] = inputs[source["input_sequence"]]
        else:
            expected[source["id"]] = scenario["program_fields"][source["program_field"]]
    return expected


def _assert_draft(draft: dict[str, Any], planning_sources: set[str]) -> None:
    candidates = draft.get("candidates", [])
    goals = draft.get("goals", [])
    if draft.get("boundary") != "ongoing" or not candidates or not goals or draft.get("evidence"):
        raise AssertionError("candidate draft does not represent a nonempty ongoing planning result")
    for collection in ["goals", "evidence", "candidates", "blockers"]:
        for entity in draft.get(collection, []):
            uuid.UUID(str(entity.get("id")))
            if not isinstance(entity.get("revision"), int) or entity["revision"] < 1 or "local" in entity:
                raise AssertionError("candidate entity did not resolve to a backend identity")
    candidate_ids = {item["id"] for item in candidates}
    goal_ids = {item["id"] for item in goals}
    if any(goal.get("source_ref_id") not in planning_sources for goal in goals):
        raise AssertionError("candidate goal escapes the exact planning-input source window")
    if {goal["resolution"]["id"] for goal in goals} - candidate_ids:
        raise AssertionError("goal resolution points outside the candidate draft")
    covered = {goal for candidate in candidates for goal in candidate.get("coverage_goal_ids", [])}
    if covered != goal_ids:
        raise AssertionError("candidate draft does not cover its exact goal set")


def _assert_ready_review(call: dict[str, Any], draft: dict[str, Any]) -> None:
    payload = call["payload"]
    review = payload.get("latest_review")
    if not isinstance(review, dict):
        raise AssertionError("Ready planning cycle has no retained review")
    candidates = {item["id"] for item in draft["candidates"]}
    decisions = review.get("candidate_decisions", [])
    if payload.get("context", {}).get("candidate_set", {}).get("status") != "ready":
        raise AssertionError("planning cycle did not reach Ready")
    if review.get("verdict") != "ready" or {item.get("candidate_id") for item in decisions} != candidates:
        raise AssertionError("Ready review does not decide the exact candidate set")
    if any(item.get("decision") != "accept" for item in decisions):
        raise AssertionError("Ready review contains a non-accepted candidate")
    if draft.get("blockers") != []:
        raise AssertionError("Ready planning cycle retains unresolved blockers")
    actions = payload.get("actions", [])
    expected = {
        "kind": "ready_call", "tool": "query",
        "arguments": {"route": "scope.candidates.context", "params": {
            "candidate_set_id": payload["context"]["candidate_set"]["id"],
            "view": "candidates", "limit": 25,
        }},
    }
    if actions != [expected]:
        raise AssertionError("Ready planning cycle exposed anything but its exact read-only inspection")
    recommended = payload.get("recommended_action")
    if recommended is not None and (recommended != 0 or actions != [expected]):
        raise AssertionError("Ready planning cycle recommends an invalid continuation")


def _assert_delta(previous: dict[str, Any], current: dict[str, Any]) -> None:
    old = {item["id"]: item for item in previous["candidates"]}
    new = {item["id"]: item for item in current["candidates"]}
    delta = current.get("delta", {})
    added = {item["candidate_id"]: item for item in delta.get("added", [])}
    changed = {item["candidate_id"]: item for item in delta.get("changed", [])}
    unchanged = {item["candidate_id"]: item for item in delta.get("unchanged", [])}
    superseded = {item["prior"]["id"]: item for item in delta.get("superseded", [])}
    if any([
        len(added) != len(delta.get("added", [])),
        len(changed) != len(delta.get("changed", [])),
        len(unchanged) != len(delta.get("unchanged", [])),
        len(superseded) != len(delta.get("superseded", [])),
    ]):
        raise AssertionError("candidate delta repeats an identity within one classification")
    groups = [set(added), set(changed), set(unchanged), set(superseded)]
    if sum(map(len, groups)) != len(set().union(*groups)):
        raise AssertionError("candidate delta classifications overlap")
    if set(added) != set(new) - set(old) or set(superseded) != set(old) - set(new):
        raise AssertionError("candidate additions or omissions are not completely classified")
    if set(changed) | set(unchanged) != set(old) & set(new):
        raise AssertionError("retained candidates are not completely classified")
    for candidate_id, item in added.items():
        uuid.UUID(candidate_id)
        if item["revision"] != 1 or item["revision"] != new[candidate_id]["revision"]:
            raise AssertionError("added candidate revision does not match the stored entity")
    for candidate_id, item in unchanged.items():
        if item["revision"] != old[candidate_id]["revision"] or new[candidate_id] != old[candidate_id]:
            raise AssertionError("unchanged candidate identity, revision, or body changed")
    for candidate_id, item in changed.items():
        if item["from_revision"] != old[candidate_id]["revision"] or item["to_revision"] != new[candidate_id]["revision"]:
            raise AssertionError("changed candidate revision transition is incoherent")
        if item["to_revision"] != item["from_revision"] + 1 or not str(item.get("rationale", "")).strip():
            raise AssertionError("changed candidate lacks one revision increment or rationale")
    for candidate_id, item in superseded.items():
        if item["prior"] != old[candidate_id] or not str(item.get("reason", "")).strip():
            raise AssertionError("supersession does not retain the exact prior entity and reason")
        if set(item.get("replacement_candidate_ids", [])) - set(new):
            raise AssertionError("supersession replacement points outside the current draft")


def _assert_history(
    first: dict[str, Any], continuations: list[dict[str, Any]], history_calls: list[dict[str, Any]],
) -> None:
    expected: dict[tuple[str, int], tuple[str, str | None, list[str]]] = {}
    current = {item["id"]: item for item in first["candidates"]}
    for draft in continuations:
        delta = draft["delta"]
        for item in delta.get("changed", []):
            expected[(item["candidate_id"], item["from_revision"])] = ("prior", None, [])
        for item in delta.get("superseded", []):
            prior = item["prior"]
            expected[(prior["id"], prior["revision"])] = (
                "superseded", item["reason"], item.get("replacement_candidate_ids", []),
            )
        for item in delta.get("added", []):
            if item["revision"] != 1:
                raise AssertionError("added candidate did not begin at revision 1")
        current = {item["id"]: item for item in draft["candidates"]}
    for item in current.values():
        expected[(item["id"], item["revision"])] = ("active", None, [])

    entries = [item["history"] for call in history_calls for item in call["payload"].get("items", [])
               if "history" in item]
    actual = {(item["candidate_id"], item["candidate_revision"]): item for item in entries}
    if len(actual) != len(entries):
        raise AssertionError("compact history repeated a candidate revision")
    for key, (status, reason, replacements) in expected.items():
        item = actual.get(key)
        if item is None:
            raise AssertionError("compact history omitted a required candidate revision")
        if item.get("status") != status:
            raise AssertionError("compact history assigned the wrong candidate status")
        if item.get("superseded_reason") != reason:
            raise AssertionError("compact history changed a supersession reason")
        if item.get("replacement_candidate_ids", []) != replacements:
            raise AssertionError("compact history changed supersession replacement identities")


def _tail_reads(calls: list[dict[str, Any]], after: int) -> list[dict[str, Any]]:
    views = {"history", "historical", "fragment", "overview"}
    reads = [
        call for call in calls[after + 1:]
        if _is(call, "query", "scope.candidates.context")
        and call["arguments"]["params"].get("view") in views
    ]
    if not reads or reads[0]["arguments"]["params"].get("view") != "history":
        raise AssertionError("historical traversal did not begin with compact history")
    _assert_offered_reads(reads)
    return reads


def _assert_cycle_bindings(
    first_context: dict[str, Any], first_ready_context: dict[str, Any],
    recorded_context: dict[str, Any], refreshed_context: dict[str, Any],
    second_context: dict[str, Any], second_ready_context: dict[str, Any],
) -> None:
    first_set = first_context["candidate_set"]
    first_ready_set = first_ready_context["candidate_set"]
    recorded_set = recorded_context["candidate_set"]
    refreshed_set = refreshed_context["candidate_set"]
    second_set = second_context["candidate_set"]
    second_ready_set = second_ready_context["candidate_set"]
    if first_set.get("input_cursor") != 1 or first_ready_set.get("revision") != first_set.get("revision", 0) + 1:
        raise AssertionError("first Ready review is not the exact next revision of input window 1")
    if recorded_set.get("revision") != first_ready_set.get("revision", 0) + 1:
        raise AssertionError("recorded amendment is not the exact next revision after first Ready")
    if recorded_set.get("latest_input") != 2 or recorded_set.get("input_cursor") != 1:
        raise AssertionError("recorded amendment did not create a stale two-input planning window")
    if refreshed_set.get("revision") != recorded_set.get("revision", 0) + 1:
        raise AssertionError("refresh is not the exact next revision after the amendment")
    if refreshed_set.get("status") != "review_required" or refreshed_set.get("input_cursor") != 1:
        raise AssertionError("refresh did not reset the prior Ready result for review")
    first_snapshot = first_context["snapshot"]
    second_snapshot = second_context["snapshot"]
    if first_ready_context["snapshot"] != first_snapshot or recorded_context["snapshot"] != first_snapshot:
        raise AssertionError("first cycle or amendment recording switched away from the original snapshot")
    if second_snapshot.get("id") == first_snapshot.get("id") or refreshed_context["snapshot"] != second_snapshot:
        raise AssertionError("amendment did not bind the second cycle to one new immutable snapshot")
    if second_ready_context["snapshot"] != second_snapshot:
        raise AssertionError("second Ready review switched away from its refreshed snapshot")
    if second_set.get("input_cursor") != 2 or second_set.get("revision", 0) <= refreshed_set.get("revision", 0):
        raise AssertionError("second draft did not consume the refreshed two-input window")
    if second_ready_set.get("revision") != second_set.get("revision", 0) + 1:
        raise AssertionError("second Ready review is not the exact next revision of its final draft")


def _amendment_refresh_index(calls: list[dict[str, Any]], record: int, ready: int) -> int:
    refreshes = [index for index, call in enumerate(calls)
                 if record < index < ready and _is(call, "command", "scope.candidates.refresh")]
    if len(refreshes) != 1:
        raise AssertionError("amendment did not lead through exactly one explicit refresh")
    return refreshes[0]


def assert_two_cycles(calls: list[dict[str, Any]], scenario: dict[str, Any]) -> dict[str, Any]:
    ready_reviews = [index for index, call in enumerate(calls) if _save_kind(call, "review")
                     and call.get("payload", {}).get("context", {}).get("candidate_set", {}).get("status") == "ready"]
    if len(ready_reviews) != 2:
        raise AssertionError("model did not produce exactly two evidenced Ready planning cycles")
    first_ready, second_ready = ready_reviews
    first_drafts = [index for index in range(first_ready) if _save_kind(calls[index], "draft")]
    if not first_drafts:
        raise AssertionError("first planning cycle has no stored draft")
    first_draft_index = first_drafts[-1]
    all_record_inputs = [index for index, call in enumerate(calls) if _is(call, "command", "scope.candidates.record_input")]
    record_inputs = [index for index in all_record_inputs if first_ready < index < second_ready]
    if len(record_inputs) != 1 or len(all_record_inputs) != 1:
        raise AssertionError("the separate amendment was not recorded exactly once after first Ready")
    record_index = record_inputs[0]
    if calls[record_index]["arguments"]["params"].get("input") != scenario["amendment"]:
        raise AssertionError("recorded amendment is not the exact supplied user text")
    refresh_index = _amendment_refresh_index(calls, record_index, second_ready)
    second_drafts = [index for index in range(refresh_index + 1, second_ready) if _save_kind(calls[index], "draft")]
    if not second_drafts:
        raise AssertionError("second planning cycle has no stored draft")
    second_draft_index = second_drafts[-1]

    first_draft = calls[first_draft_index]["payload"]["draft"]
    second_draft = calls[second_draft_index]["payload"]["draft"]
    first_context = calls[first_draft_index]["payload"]["context"]
    second_context = calls[second_draft_index]["payload"]["context"]
    first_snapshot = first_context["snapshot"]
    second_snapshot = second_context["snapshot"]
    for context in [first_context, second_context]:
        if context["candidate_set"]["program_id"] != scenario["program_id"]:
            raise AssertionError("candidate cycle is not bound to the expected Program")
        if context["snapshot"]["selected_worktree_ids"] != [scenario["worktree_id"]]:
            raise AssertionError("candidate cycle is not bound to the selected owned source")
    _assert_draft(first_draft, {item["id"] for item in first_snapshot["source_refs"] if item["kind"] == "planning_input"})
    _assert_draft(second_draft, {item["id"] for item in second_snapshot["source_refs"] if item["kind"] == "planning_input"})
    _assert_ready_review(calls[first_ready], first_draft)
    _assert_ready_review(calls[second_ready], second_draft)
    recorded_context = calls[record_index]["payload"]["context"]
    refreshed_context = calls[refresh_index]["payload"]["context"]
    _assert_cycle_bindings(
        first_context, calls[first_ready]["payload"]["context"], recorded_context,
        refreshed_context, second_context, calls[second_ready]["payload"]["context"],
    )
    prior_draft = first_draft
    for index in second_drafts:
        current_draft = calls[index]["payload"]["draft"]
        _assert_delta(prior_draft, current_draft)
        prior_draft = current_draft

    source_setup = _assert_source_setup(calls, first_drafts[0], scenario)
    first_navigation = [call for call in calls[:first_drafts[0]]
                        if call["tool"] in {"get_state", "help", "query", "command"}]
    if not first_navigation or first_navigation[0]["tool"] != "get_state" or first_navigation[0]["arguments"]:
        raise AssertionError("model did not start recovery from get_state")
    opened = [call for call in first_navigation if _is(call, "command", "workspace.open")]
    if len(opened) != 1:
        raise AssertionError("new child session did not execute exactly one offered workspace.open")
    _assert_offered_reads(first_navigation, explicit=source_setup)
    first_reads = [call for call in first_navigation if call["tool"] in {"get_state", "query"}]
    first_overview = next(call["payload"] for call in first_reads
                          if call["tool"] == "query" and call["arguments"]["params"].get("view") == "overview")
    if first_overview["context"]["snapshot"] != first_snapshot:
        raise AssertionError("first context traversal switched away from its stored draft snapshot")
    if _fragments(first_reads, None) != _expected_bodies(first_overview["context"]["snapshot"], scenario, {1: scenario["planning_input"]}):
        raise AssertionError("first planning context was not read in full")

    _assert_command_template(calls, first_drafts[0], [call["payload"] for call in first_reads], "draft")
    _assert_command_template(calls, first_ready, [call["payload"] for call in calls[first_draft_index:first_ready]], "review")
    _assert_command_template(calls, record_index, [calls[first_ready]["payload"]], "input")
    _assert_command_template(calls, refresh_index, [calls[record_index]["payload"]], None)

    second_reads = [call for call in calls[refresh_index + 1:second_drafts[0]] if call["tool"] == "query"]
    _assert_offered_reads(second_reads, calls[refresh_index]["payload"])
    second_overview = next(call["payload"] for call in second_reads if call["arguments"]["params"].get("view") == "overview")
    if second_overview["context"]["snapshot"] != second_snapshot:
        raise AssertionError("second context traversal switched away from the refreshed snapshot")
    inputs = [item["input"]["sequence"] for call in second_reads
              if call["arguments"]["params"].get("view") == "inputs" for item in call["payload"].get("items", [])]
    if inputs != [1, 2]:
        raise AssertionError("second planning context did not expose the exact two-input window")
    expected_current = _expected_bodies(second_overview["context"]["snapshot"], scenario, {1: scenario["planning_input"], 2: scenario["amendment"]})
    if _fragments(second_reads, None) != expected_current:
        raise AssertionError("second planning context was not read in full")
    _assert_command_template(calls, second_drafts[0], [call["payload"] for call in second_reads], "draft")
    _assert_command_template(calls, second_ready, [call["payload"] for call in calls[second_draft_index:second_ready]], "review")

    tail = _tail_reads(calls, second_ready)
    history = [call for call in tail if _is(call, "query", "scope.candidates.context")
               and call["arguments"]["params"].get("view") == "history"]
    historical = [call for call in tail if _is(call, "query", "scope.candidates.context")
                  and call["arguments"]["params"].get("view") == "historical"]
    first_revision = calls[first_draft_index]["payload"]["context"]["candidate_set"]["revision"]
    if not history or not historical or any(call["arguments"]["params"].get("draft_revision") != first_revision for call in historical):
        raise AssertionError("compact history and the original historical draft were not read")
    if history[-1]["payload"].get("next_after") is not None or historical[-1]["payload"].get("next_after") is not None:
        raise AssertionError("history traversal stopped before its terminal page")
    historical_snapshot = historical[0]["payload"]["historical"]["snapshot"]
    if historical_snapshot != first_snapshot or any(call["payload"]["historical"]["snapshot"] != historical_snapshot for call in historical):
        raise AssertionError("historical paging did not retain the exact original snapshot")
    if any(call["payload"]["historical"]["set_revision"] != first_revision for call in historical):
        raise AssertionError("historical paging did not retain the requested original draft revision")
    if any(call["payload"]["historical"]["input_cursor"] != 1 for call in historical):
        raise AssertionError("historical paging did not retain the original input cursor")
    historical_fragments = [call for call in tail if _is(call, "query", "scope.candidates.context")
                            and call["arguments"]["params"].get("view") == "fragment"
                            and call["arguments"]["params"].get("draft_revision") == first_revision]
    if any(action.get("tool") == "command" for call in historical + historical_fragments
           for action in call["payload"].get("actions", [])):
        raise AssertionError("historical reads exposed a mutation template")
    old_candidates = {item["id"] for item in first_draft["candidates"]}
    seen_old = {item["candidate"]["id"] for call in historical for item in call["payload"].get("items", []) if "candidate" in item}
    if seen_old != old_candidates:
        raise AssertionError("historical paging omitted or added original candidates")
    expected_old = _expected_bodies(historical_snapshot, scenario, {1: scenario["planning_input"]})
    if _fragments(tail, first_revision) != expected_old:
        raise AssertionError("original historical source bodies were not read in full")
    _assert_history(first_draft, [calls[index]["payload"]["draft"] for index in second_drafts], history)
    historical_ids = {id(call) for call in historical + historical_fragments}
    last_historical = max(index for index, call in enumerate(tail) if id(call) in historical_ids)
    current_reads = [call for call in tail[last_historical + 1:]
                     if call["arguments"]["params"].get("view") == "overview"]
    if not current_reads or current_reads[-1]["payload"]["context"]["snapshot"]["id"] != second_snapshot["id"]:
        raise AssertionError("historical traversal did not return to the current head")
    if current_reads[-1]["payload"]["context"]["candidate_set"]["revision"] != calls[second_ready]["payload"]["context"]["candidate_set"]["revision"]:
        raise AssertionError("historical traversal returned to an unexpected candidate-set revision")
    _assert_source_snapshots(calls, scenario)
    return {
        "draft_receipts": [(calls[index]["arguments"], calls[index]["payload"]) for index in [first_draft_index, second_draft_index]],
        "final_payload": calls[second_ready]["payload"], "first_draft_revision": first_revision,
        "first_snapshot": first_snapshot, "second_snapshot": second_snapshot,
    }
