"""Live parent metadata and private App Server event evidence."""
from __future__ import annotations

import hashlib
import json
import os
import pathlib
import uuid
from typing import Any

PARENT_COLLAB_TOOLS = {"spawnAgent", "wait", "sendInput", "resumeAgent"}
PARENT_PASSIVE_ITEMS = {"userMessage", "hookPrompt", "agentMessage", "plan", "reasoning", "contextCompaction"}
LINEAGE_FIELDS = {
    "id", "type", "status", "tool", "senderThreadId", "receiverThreadIds",
    "agentThreadId", "model", "reasoningEffort",
}


class RawEventLog:
    """Append every native event once to a private file."""

    def __init__(self, proof_path: pathlib.Path):
        suffix = uuid.uuid4().hex
        self.path = proof_path.with_name(f"{proof_path.stem}.model-events-{suffix}.jsonl")
        descriptor = os.open(self.path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
        self._stream = os.fdopen(descriptor, "wb", buffering=0)
        self._digest = hashlib.sha256()
        self.count = 0

    def append(self, event: dict[str, Any]) -> None:
        record = json.dumps(
            {"sequence": self.count + 1, "event": event},
            ensure_ascii=False, separators=(",", ":"),
        ).encode("utf-8") + b"\n"
        self._stream.write(record)
        os.fsync(self._stream.fileno())
        self._digest.update(record)
        self.count += 1

    def evidence(self) -> dict[str, Any]:
        return {"path": str(self.path), "count": self.count, "sha256": self._digest.hexdigest()}

    def close(self) -> None:
        self._stream.close()


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


def loaded_thread_ids(app: Any) -> tuple[list[str], list[dict[str, Any]]]:
    pages = _pages(app, "thread/loaded/list", {"limit": 100})
    values = [value for page in pages for value in page["response"].get("data", [])]
    if any(not isinstance(value, str) for value in values) or len(values) != len(set(values)):
        raise AssertionError("loaded thread inventory is malformed or duplicated")
    return values, pages


def read_thread_metadata(app: Any, thread_id: str) -> dict[str, Any]:
    request = {"threadId": thread_id, "includeTurns": False}
    return {"request": request, "response": app.request("thread/read", request)}


def child_ids(items: list[dict[str, Any]]) -> set[str]:
    found = set()
    for item in items:
        if item.get("type") == "subAgentActivity" and item.get("agentThreadId"):
            found.add(item["agentThreadId"])
        if item.get("type") == "collabAgentToolCall" and item.get("tool") == "spawnAgent":
            found.update(item.get("receiverThreadIds", []))
    return found


def public_lineage_item(item: dict[str, Any]) -> dict[str, Any]:
    return {key: item[key] for key in LINEAGE_FIELDS if key in item}


def record_parent_item(capture: dict[str, Any], parent_thread_id: str, event: dict[str, Any]) -> bool:
    params = event.get("params", {})
    item = params.get("item") if isinstance(params, dict) else None
    if params.get("threadId") != parent_thread_id or not isinstance(item, dict):
        return False
    public = public_lineage_item(item)
    records = capture.setdefault("lineage_event_items", [])
    for index, existing in enumerate(records):
        if existing.get("id") == public.get("id") and existing.get("type") == public.get("type"):
            records[index] = public
            break
    else:
        records.append(public)
    return True


def validated_thread_metadata(read: dict[str, Any], thread_id: str, parent_id: str | None) -> dict[str, Any]:
    metadata = read.get("response", {}).get("thread", {})
    if (
        metadata.get("id") != thread_id or metadata.get("parentThreadId") != parent_id
        or metadata.get("model") != "gpt-5.6-sol" or metadata.get("reasoningEffort") != "medium"
    ):
        raise AssertionError("thread metadata does not match the approved Sol medium lineage")
    return metadata


def refresh_lineage(app: Any, parent_thread_id: str, capture: dict[str, Any], proof: Any) -> None:
    parent = read_thread_metadata(app, parent_thread_id)
    parent_metadata = validated_thread_metadata(parent, parent_thread_id, None)
    observed = child_ids(capture.get("lineage_event_items", []))
    loaded, pages = loaded_thread_ids(app)
    lineage = {
        "parent": parent, "observed_child_ids": sorted(observed),
        "loaded_thread_ids": loaded, "loaded_pages": pages,
        "child_metadata_observations": capture.get("lineage", {}).get("child_metadata_observations", []),
        "parent_metadata_observations": capture.get("lineage", {}).get("parent_metadata_observations", []),
    }
    parent_observation = {
        "id": parent_metadata.get("id"), "parentThreadId": parent_metadata.get("parentThreadId"),
        "model": parent_metadata.get("model"), "reasoningEffort": parent_metadata.get("reasoningEffort"),
        "ephemeral": parent_metadata.get("ephemeral"), "status": parent_metadata.get("status"),
    }
    if not lineage["parent_metadata_observations"] \
            or lineage["parent_metadata_observations"][-1] != parent_observation:
        lineage["parent_metadata_observations"].append(parent_observation)
    if len(observed) == 1:
        child_id = next(iter(observed))
        try:
            child = read_thread_metadata(app, child_id)
        except Exception as error:
            detail = getattr(error, "error", None)
            if not isinstance(detail, dict) or detail.get("code") != -32600:
                raise
            lineage["child_metadata_pending"] = detail
        else:
            metadata = validated_thread_metadata(child, child_id, parent_thread_id)
            observation = {
                "id": metadata.get("id"), "parentThreadId": metadata.get("parentThreadId"),
                "model": metadata.get("model"), "reasoningEffort": metadata.get("reasoningEffort"),
                "ephemeral": metadata.get("ephemeral"), "status": metadata.get("status"),
            }
            prior = lineage["child_metadata_observations"]
            if not prior or prior[-1] != observation:
                prior = [*prior, observation]
            lineage.update({"child": child, "child_metadata_observations": prior})
    capture["lineage"] = lineage
    proof.persist()


def _wire_actor(pair: dict[str, Any]) -> str | None:
    params = pair.get("request", {}).get("params", {})
    metadata = params.get("_meta") if isinstance(params, dict) else None
    return metadata.get("threadId") if isinstance(metadata, dict) else None


def _successful_get_state(pair: dict[str, Any]) -> bool:
    params = pair.get("request", {}).get("params", {})
    response = pair.get("response", {})
    result = response.get("result") if isinstance(response, dict) else None
    return (
        pair.get("response_source") == "mcp_wire" and pair.get("forwarded") is True
        and params.get("name") == "get_state" and params.get("arguments", {}) == {}
        and isinstance(result, dict) and result.get("isError") is not True
        and isinstance(result.get("content"), list)
    )


def try_open_identity_gate(
    app: Any, parent_thread_id: str, capture: dict[str, Any], proof: Any, fixture: Any,
) -> bool:
    refresh_lineage(app, parent_thread_id, capture, proof)
    lineage = capture["lineage"]
    ids = lineage["observed_child_ids"]
    if len(ids) > 1:
        raise AssertionError("model phase exposed more than one child")
    if len(ids) != 1 or "child" not in lineage:
        return False
    child_id = ids[0]
    metadata = validated_thread_metadata(lineage["child"], child_id, parent_thread_id)
    loaded = set(lineage["loaded_thread_ids"])
    if loaded - {parent_thread_id, child_id}:
        raise AssertionError("loaded actor inventory contains an unexpected thread")
    assert_parent_boundary(capture)
    pairs, errors = fixture.guarded_wire_pairs(allow_pending=True)
    if errors:
        raise AssertionError("MCP wire capture is malformed before the identity gate")
    if not pairs:
        return False
    first = pairs[0]
    if _wire_actor(first) != child_id or not _successful_get_state(first):
        raise AssertionError("first completed MCP call is not the approved child's successful get_state")
    if any(_wire_actor(pair) != child_id for pair in pairs):
        raise AssertionError("MCP wire capture contains a call by an unexpected actor")
    fixture.open_model_gate(parent_thread_id, child_id)
    capture["identity_gate"] = {
        "status": "open", "parent_thread_id": parent_thread_id, "child_thread_id": child_id,
        "first_call": {
            "source": "mcp_wire", "connection_id": first["connection_id"],
            "request_sequence": first["request_sequence"],
            "request_raw_sha256": first["request_raw_sha256"],
            "response_raw_sha256": first["response_raw_sha256"],
        },
        "loaded_thread_ids": sorted(loaded), "child_metadata": metadata,
    }
    proof.persist()
    return True


def assert_one_sol_child(capture: dict[str, Any], parent_thread_id: str) -> dict[str, Any]:
    lineage = capture.get("lineage", {})
    ids = lineage.get("observed_child_ids", [])
    if len(ids) != 1 or "child" not in lineage:
        raise AssertionError("approved model turn did not expose exactly one child")
    child_id = ids[0]
    metadata = lineage["child"]["response"].get("thread", {})
    observations = lineage.get("child_metadata_observations", [])
    parent_observations = lineage.get("parent_metadata_observations", [])
    validated_thread_metadata(lineage.get("parent", {}), parent_thread_id, None)
    if (
        metadata.get("id") != child_id or metadata.get("parentThreadId") != parent_thread_id
        or not observations or not parent_observations
        or any(item.get("model") != "gpt-5.6-sol"
                                   or item.get("reasoningEffort") != "medium" for item in observations)
        or any(item.get("id") != parent_thread_id or item.get("parentThreadId") is not None
               or item.get("model") != "gpt-5.6-sol" or item.get("reasoningEffort") != "medium"
               for item in parent_observations)
    ):
        raise AssertionError("child lineage or configured model evidence changed")
    loaded = set(lineage.get("loaded_thread_ids", []))
    if loaded - {parent_thread_id, child_id}:
        raise AssertionError("completion actor inventory contains an unexpected thread")
    spawns = [item for item in capture.get("lineage_event_items", [])
              if item.get("type") == "collabAgentToolCall" and item.get("tool") == "spawnAgent"]
    if spawns:
        matching = [item for item in spawns if item.get("receiverThreadIds") == [child_id]]
        if len(spawns) != 1 or len(matching) != 1:
            raise AssertionError("observed spawn request is ambiguous")
        spawn = matching[0]
        if (spawn.get("senderThreadId") != parent_thread_id or spawn.get("model") != "gpt-5.6-sol"
                or spawn.get("reasoningEffort") != "medium"):
            raise AssertionError("observed spawn request differs from approved child configuration")
    return {
        "child_thread_id": child_id, "configured_model_evidence": observations,
        "configured_model_is_per_turn_telemetry": False,
        "spawn_request_observed": bool(spawns),
        "loaded_child_observed": child_id in loaded,
        "child_non_mcp_actions_observable": False,
    }


def assert_parent_boundary(capture: dict[str, Any]) -> None:
    child_id = capture["lineage"]["observed_child_ids"][0]
    for item in capture.get("lineage_event_items", []):
        kind = item.get("type")
        if kind == "subAgentActivity":
            if item.get("agentThreadId") != child_id:
                raise AssertionError("parent observed activity outside the approved child")
            continue
        if kind == "collabAgentToolCall" and item.get("tool") in PARENT_COLLAB_TOOLS:
            receivers = set(item.get("receiverThreadIds", []))
            if receivers - {child_id} or (item.get("tool") in {"spawnAgent", "sendInput", "resumeAgent"}
                                         and receivers != {child_id}):
                raise AssertionError("parent collaboration targeted an actor outside the approved child")
            continue
        if kind in PARENT_PASSIVE_ITEMS:
            continue
        raise AssertionError("parent used a practical action outside one-child collaboration")
    actors = set(capture.get("observed_actor_thread_ids", []))
    if actors - {capture["thread_id"], child_id}:
        raise AssertionError("model phase emitted events for an actor outside the approved lineage")
