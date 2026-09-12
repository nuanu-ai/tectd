"""Exact backend action, recovery, and safe read-lineage assertions."""
from __future__ import annotations

from typing import Any


def _is(call: dict[str, Any], tool: str, route: str | None = None) -> bool:
    return call.get("tool") == tool and (
        route is None or call.get("arguments", {}).get("route") == route
    )


def _ready(payload: dict[str, Any]) -> list[tuple[str, dict[str, Any]]]:
    return [
        (action["tool"], action["arguments"])
        for action in payload.get("actions", [])
        if action.get("kind") in {"ready_call", "needs_input"}
        and isinstance(action.get("tool"), str)
        and isinstance(action.get("arguments"), dict)
    ]


def _is_current_overview(call: tuple[str, dict[str, Any]]) -> bool:
    tool, arguments = call
    params = arguments.get("params", {})
    return (
        tool == "query"
        and arguments.get("route") == "scope.candidates.context"
        and set(arguments) == {"route", "params"}
        and set(params) == {"candidate_set_id", "view", "limit"}
        and isinstance(params.get("candidate_set_id"), str)
        and params.get("view") == "overview"
        and params.get("limit") == 25
    )


def reusable_overviews(payloads: list[dict[str, Any]]) -> list[tuple[str, dict[str, Any]]]:
    return [call for payload in payloads for call in _ready(payload) if _is_current_overview(call)]


def assert_offered_reads(
    reads: list[dict[str, Any]], initial: dict[str, Any] | None = None,
    explicit: list[tuple[str, dict[str, Any]]] | None = None,
    reusable: list[tuple[str, dict[str, Any]]] | None = None,
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
    reusable_calls = [*(reusable or []), *(call for call in offered if _is_current_overview(call))]
    for index, read in enumerate(reads):
        call = (read["tool"], read["arguments"])
        supplied_call = False
        if call in supplied:
            supplied.remove(call)
            supplied_call = True
        if (
            read["tool"] != "help" and not supplied_call and call not in reusable_calls
            and (initial is not None or index)
        ):
            try:
                offered.pop(offered.index(call))
            except ValueError as error:
                raise AssertionError(
                    "model reconstructed a context read instead of using a backend action"
                ) from error
        emitted = calls(read["payload"])
        offered.extend(emitted)
        reusable_calls.extend(item for item in emitted if _is_current_overview(item))
    if supplied:
        raise AssertionError("model omitted an explicitly supplied fixture setup call")


def _matches_authored_template(
    action: dict[str, Any], actual: dict[str, Any], params: dict[str, Any], variable: str,
) -> bool:
    arguments = action["arguments"]
    expected = arguments.get("params", {})
    fields = [field.get("path") for field in action.get("input", {}).get("fields", [])]
    prefix = f"arguments.params.{variable}"
    if (
        action.get("kind") != "needs_input"
        or arguments.get("route") != actual["arguments"].get("route")
        or not any(path == prefix or str(path).startswith(prefix + ".") for path in fields)
        or set(params) != set(expected) | ({variable} if variable not in expected else set())
        or any(params.get(key) != value for key, value in expected.items() if key != variable)
    ):
        return False
    if variable not in expected:
        return variable in params
    if variable != "review" or not isinstance(expected[variable], dict) or not isinstance(params[variable], dict):
        return False
    backend_reviews = expected[variable].get("protected_change_reviews", [])
    actual_reviews = params[variable].get("protected_change_reviews", [])
    if len(backend_reviews) != len(actual_reviews):
        return False
    for backend, authored in zip(backend_reviews, actual_reviews, strict=True):
        if any(authored.get(key) != value for key, value in backend.items()):
            return False
        if set(authored) - set(backend) - {"rationale"}:
            return False
    allowed = {str(path).removeprefix(prefix + ".").split(".", 1)[0] for path in fields}
    return set(params[variable]) - set(expected[variable]) <= allowed


def assert_command_template(
    calls: list[dict[str, Any]], index: int, payloads: list[dict[str, Any]], variable: str | None,
) -> None:
    actual = calls[index]
    candidates = [
        action for payload in payloads for action in payload.get("actions", [])
        if action.get("kind") in {"ready_call", "needs_input"}
        and action.get("tool") == actual["tool"]
        and isinstance(action.get("arguments"), dict)
    ]
    if variable is None:
        matched = actual["arguments"] in [action["arguments"] for action in candidates]
    else:
        params = actual["arguments"].get("params", {})
        matched = any(_matches_authored_template(action, actual, params, variable) for action in candidates)
    if not matched:
        raise AssertionError("model did not preserve the backend-provided command template")


def successful(call: dict[str, Any]) -> bool:
    return call.get("status", "completed") == "completed" and not call.get("is_error", False)


def assert_failed_body_recoveries(calls: list[dict[str, Any]]) -> None:
    for index, failed in enumerate(calls):
        params = failed.get("arguments", {}).get("params", {})
        kind = params.get("kind")
        if successful(failed) or not _is(failed, "command", "scope.candidates.save") or kind not in {"draft", "review"}:
            continue
        templates = [
            (payload, action)
            for prior in calls[:index]
            for payload in [prior.get("payload", {})]
            for action in payload.get("actions", [])
            if _matches_authored_template(action, failed, params, kind)
        ]
        if not templates:
            raise AssertionError("rejected candidate body did not preserve a backend template")
        template_payload, template = templates[-1]
        if index + 1 >= len(calls):
            raise AssertionError("rejected candidate body omitted its state recovery checkpoint")
        checkpoint = calls[index + 1]
        summaries = checkpoint.get("payload", {}).get("candidate_sets", [])
        summary = next((item for item in summaries if item.get("id") == params.get("candidate_set_id")), None)
        before = template_payload.get("context", {}).get("candidate_set", {})
        if (
            checkpoint.get("tool") != "get_state" or checkpoint.get("arguments") != {}
            or not successful(checkpoint) or summary is None
            or summary.get("revision") != params.get("revision")
            or summary.get("snapshot_id") != params.get("snapshot_id")
            or summary.get("status") != before.get("status")
        ):
            raise AssertionError("rejected candidate body changed state before its checkpoint")
        corrected = [
            call for call in calls[index + 1:]
            if successful(call) and _is(call, "command", "scope.candidates.save")
            and call.get("arguments", {}).get("params", {}).get("kind") == kind
            and _matches_authored_template(template, call, call["arguments"]["params"], kind)
        ]
        if not corrected or corrected[0]["arguments"]["params"].get(kind) == params.get(kind):
            raise AssertionError("rejected candidate body was not corrected under the same backend controls")
