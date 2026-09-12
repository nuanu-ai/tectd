"""Live parent/child App Server evidence capture for the native model phase."""
from __future__ import annotations

from typing import Any

PASSIVE_ITEMS = {"userMessage", "agentMessage", "plan", "reasoning", "contextCompaction"}
PARENT_COLLAB_TOOLS = {"spawnAgent", "wait", "sendInput", "resumeAgent"}


def _pages(app: Any, method: str, params: dict[str, Any]) -> list[dict[str, Any]]:
    pages, cursor, seen = [], None, set()
    while True:
        request = dict(params)
        if cursor is not None:
            request["cursor"] = cursor
        response = app.request(method, request)
        pages.append({"request": request, "response": response})
        cursor = response.get("nextCursor")
        if cursor is None:
            return pages
        if cursor in seen:
            raise AssertionError(method + " returned a repeated pagination cursor")
        seen.add(cursor)


def read_thread(app: Any, thread_id: str) -> dict[str, Any]:
    metadata_request = {"threadId": thread_id, "includeTurns": False}
    metadata = app.request("thread/read", metadata_request)
    turns = _pages(app, "thread/turns/list", {
        "threadId": thread_id, "sortDirection": "asc", "itemsView": "full", "limit": 100,
    })
    items = _pages(app, "thread/items/list", {
        "threadId": thread_id, "sortDirection": "asc", "limit": 100,
    })
    return {"metadata_request": metadata_request, "metadata": metadata, "turn_pages": turns, "item_pages": items}


def latest_items(snapshot: dict[str, Any]) -> list[dict[str, Any]]:
    latest: dict[tuple[str, str], dict[str, Any]] = {}
    order: list[tuple[str, str]] = []
    for page in snapshot["item_pages"]:
        for entry in page["response"].get("data", []):
            item = entry.get("item", {})
            key = (str(entry.get("turnId")), str(item.get("id")))
            if key not in latest:
                order.append(key)
            latest[key] = entry
    return [latest[key] for key in order]


def event_items(events: list[dict[str, Any]]) -> list[dict[str, Any]]:
    return [event.get("params", {}).get("item", {}) for event in events if event.get("params", {}).get("item")]


def child_ids(items: list[dict[str, Any]]) -> set[str]:
    found = set()
    for item in items:
        if item.get("type") == "subAgentActivity" and item.get("agentThreadId"):
            found.add(item["agentThreadId"])
        if item.get("type") == "collabAgentToolCall" and item.get("tool") == "spawnAgent":
            found.update(item.get("receiverThreadIds", []))
    return found


def refresh_lineage(app: Any, parent_thread_id: str, capture: dict[str, Any], proof: Any) -> None:
    parent = read_thread(app, parent_thread_id)
    parent_entries = latest_items(parent)
    observed = child_ids(event_items(capture["events"]) + [entry["item"] for entry in parent_entries])
    prior = capture.get("lineage", {}).get("child_metadata_observations", [])
    capture["lineage"] = {"parent": parent, "observed_child_ids": sorted(observed)}
    proof.persist()
    if len(observed) == 1:
        child_id = next(iter(observed))
        child = read_thread(app, child_id)
        metadata = child.get("metadata", {}).get("thread", {})
        capture["lineage"].update({
            "child": child,
            "child_metadata_observations": [*prior, {
                "id": metadata.get("id"), "parentThreadId": metadata.get("parentThreadId"),
                "model": metadata.get("model"), "reasoningEffort": metadata.get("reasoningEffort"),
                "ephemeral": metadata.get("ephemeral"), "status": metadata.get("status"),
            }],
        })
    proof.persist()


def child_items(capture: dict[str, Any]) -> list[dict[str, Any]]:
    child = capture.get("lineage", {}).get("child")
    return [entry["item"] for entry in latest_items(child)] if child else []


def assert_one_sol_child(capture: dict[str, Any], parent_thread_id: str) -> dict[str, Any]:
    lineage = capture.get("lineage", {})
    ids = lineage.get("observed_child_ids", [])
    if len(ids) != 1 or "child" not in lineage:
        raise AssertionError("approved model turn did not expose exactly one child")
    child_id, child = ids[0], lineage["child"]
    metadata = child["metadata"].get("thread", {})
    if metadata.get("id") != child_id or metadata.get("parentThreadId") != parent_thread_id:
        raise AssertionError("child lineage does not bind to the exact test parent")
    observations = lineage.get("child_metadata_observations", [])
    if not observations or any(
        item.get("model") != "gpt-5.6-sol" or item.get("reasoningEffort") != "medium" for item in observations
    ):
        raise AssertionError("child configured model or effort is missing, changed, or unexpected")
    parent_items = [entry["item"] for entry in latest_items(lineage["parent"])]
    spawns = [item for item in parent_items if item.get("type") == "collabAgentToolCall" and item.get("tool") == "spawnAgent"]
    matching = [item for item in spawns if item.get("receiverThreadIds") == [child_id]]
    if len(spawns) != 1 or len(matching) != 1:
        raise AssertionError("exact parent spawn request is unavailable or ambiguous")
    spawn = matching[0]
    if spawn.get("senderThreadId") != parent_thread_id or spawn.get("model") != "gpt-5.6-sol" or spawn.get("reasoningEffort") != "medium":
        raise AssertionError("spawn request does not prove the approved Sol medium child")
    if child_ids(child_items(capture)):
        raise AssertionError("approved child created a descendant")
    return {
        "child_thread_id": child_id,
        "configured_model_evidence": observations,
        "configured_model_is_per_turn_telemetry": False,
        "spawn_requested_model": spawn.get("model"),
        "spawn_requested_effort": spawn.get("reasoningEffort"),
    }


def assert_parent_boundary(capture: dict[str, Any]) -> None:
    parent = [entry["item"] for entry in latest_items(capture["lineage"]["parent"])]
    child_id = capture["lineage"]["observed_child_ids"][0]
    for item in parent:
        kind = item.get("type")
        if kind in PASSIVE_ITEMS or kind == "subAgentActivity":
            continue
        if kind == "collabAgentToolCall" and item.get("tool") in PARENT_COLLAB_TOOLS:
            receivers = set(item.get("receiverThreadIds", []))
            if receivers - {child_id} or (item.get("tool") in {"spawnAgent", "sendInput", "resumeAgent"} and receivers != {child_id}):
                raise AssertionError("parent collaboration targeted an actor outside the approved child")
            continue
        raise AssertionError("parent used a practical action outside the approved one-child collaboration")

    actors = {
        event.get("params", {}).get("threadId") for event in capture.get("all_events", [])
        if event.get("params", {}).get("threadId") is not None
    }
    if not actors <= {capture["thread_id"], child_id}:
        raise AssertionError("model phase emitted events for an actor outside the approved lineage")


def assert_child_item_boundary(items: list[dict[str, Any]]) -> None:
    unexpected = sorted({str(item.get("type")) for item in items} - PASSIVE_ITEMS - {"mcpToolCall"})
    if unexpected:
        raise AssertionError("child used disallowed item types: " + ", ".join(unexpected))
