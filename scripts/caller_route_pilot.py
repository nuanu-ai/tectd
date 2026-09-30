"""Source-only S05 caller dispatch pilot: scripted offline host only.

Production lacks a trusted S05 receipt reader, host tool-surface attestor and
live authorization issuer. This module cannot make a paid App Server call.
At-most-once reservation is per stable ledger directory, not global; a future
live integration must own one stable ledger path across restarts and callers.
"""

from __future__ import annotations

import hashlib
import json
import os
import stat
from copy import deepcopy
from datetime import datetime, timezone
from pathlib import Path
from typing import Any


class DispatchRejected(ValueError):
    """No task was dispatched."""


class OfflineMockHost:
    """Scripted JSON-RPC replies; no socket, process, network or delegate."""

    def __init__(self, replies: list[tuple[str, Any]], *, tool_surface: dict[str, Any]):
        self.replies = list(replies)
        self.surface = deepcopy(tool_surface)
        self.calls: list[tuple[str, dict[str, Any]]] = []

    def attest_tool_surface(self) -> dict[str, Any]:
        return deepcopy(self.surface)

    def request(self, method: str, params: dict[str, Any]) -> dict[str, Any]:
        self.calls.append((method, deepcopy(params)))
        if not self.replies:
            raise AssertionError("unscripted host request")
        expected, reply = self.replies.pop(0)
        if method != expected:
            raise AssertionError(f"expected {expected}, got {method}")
        if isinstance(reply, Exception):
            raise reply
        return deepcopy(reply)


class OfflineSelectionSource:
    """Separate stored fixture record and catalogue, never caller flags.

    It is accepted only with OfflineMockHost. It is not a production authority
    or an independently authenticated S05 disposition receipt.
    """

    def __init__(self, records: dict[str, dict[str, Any]], routes: dict[str, dict[str, Any]]):
        self.records = deepcopy(records)
        self.routes = deepcopy(routes)

    def verify(self, request: dict[str, str], now: datetime) -> tuple[dict[str, Any], dict[str, Any]]:
        decision_id = _required(request.get("decision_id"), "decision_id")
        record = self.records.get(decision_id)
        if not isinstance(record, dict) or record.get("decision_id") != decision_id:
            raise DispatchRejected("no independently stored decision")
        if record.get("disposition") != "accept" or record.get("advice_status") != "selected":
            raise DispatchRejected("decision has no accepted selection")
        for field in ("preparation_digest", "work_revision", "capability_digest", "catalogue_version",
                      "requested_route", "selected_route", "model", "effort"):
            _required(record.get(field), field)
        if _utc(record.get("expires_at")) <= now.astimezone(timezone.utc):
            raise DispatchRejected("selection is stale")
        for field in ("preparation_digest", "work_revision", "capability_digest", "catalogue_version"):
            if request.get(field) != record[field]:
                raise DispatchRejected(f"caller {field} differs from stored decision")
        if record["requested_route"] != record["selected_route"] and record.get("override_authorized") is not True:
            raise DispatchRejected("requested route override lacks authority")
        route = self.routes.get(record["selected_route"])
        if not isinstance(route, dict) or route.get("allowed") is not True:
            raise DispatchRejected("selected route is not allowed")
        for field in ("catalogue_version", "capability_digest", "model", "effort"):
            if route.get(field) != record[field]:
                raise DispatchRejected(f"route {field} differs from decision")
        return deepcopy(record), deepcopy(route)


class OfflinePilotPermit:
    """Explicit authority to exercise one scripted fixture, never a live host."""

    def __init__(self, invocation_key: str):
        self.invocation_key = _required(invocation_key, "invocation_key")


def _required(value: Any, name: str) -> str:
    if not isinstance(value, str) or not value or value.strip() != value:
        raise DispatchRejected(f"{name} must be a nonempty exact string")
    return value


def _utc(value: Any) -> datetime:
    try:
        parsed = datetime.fromisoformat(_required(value, "expires_at").replace("Z", "+00:00"))
    except ValueError as error:
        raise DispatchRejected("invalid expires_at") from error
    if parsed.tzinfo is None:
        raise DispatchRejected("expires_at needs a timezone")
    return parsed.astimezone(timezone.utc)


def _ledger(directory: Path) -> Path:
    if directory.is_symlink():
        raise DispatchRejected("ledger directory cannot be a symlink")
    try:
        directory.mkdir(mode=0o700)
    except FileExistsError:
        pass
    info = directory.lstat()
    if not stat.S_ISDIR(info.st_mode) or info.st_uid != os.getuid() or info.st_mode & 0o077:
        raise DispatchRejected("ledger directory must be owner-only")
    return directory


def _append_stage(directory: Path, key_digest: str, index: int, stage: str, **fields: Any) -> None:
    path = directory / f"{key_digest}.{index:02d}-{stage}.json"
    descriptor = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
    with os.fdopen(descriptor, "w", encoding="utf-8") as stream:
        json.dump({"stage": stage, **fields}, stream, sort_keys=True)
        stream.write("\n")
        stream.flush()
        os.fsync(stream.fileno())
    directory_fd = os.open(directory, os.O_RDONLY)
    try:
        os.fsync(directory_fd)
    finally:
        os.close(directory_fd)


def _read_thread(result: Any, thread_id: str, model: str, effort: str) -> dict[str, Any]:
    metadata = result.get("thread") if isinstance(result, dict) else None
    if not isinstance(metadata, dict) or metadata.get("id") != thread_id:
        raise RuntimeError("host thread metadata identity mismatch")
    if metadata.get("model") != model or metadata.get("reasoningEffort") != effort:
        raise RuntimeError("host configured model or effort mismatch")
    return metadata


def dispatch_once(
    source: OfflineSelectionSource, host: OfflineMockHost, request: dict[str, str],
    ledger_dir: Path, permit: OfflinePilotPermit, *, now: datetime | None = None,
) -> dict[str, Any]:
    """Exercise one scripted task per key per stable ledger directory.

    Unknown send is never retried. A different ledger directory has a
    different key namespace; live integration must own one stable path.

    Raw dictionaries, marker booleans and arbitrary clients cannot authorize
    dispatch. Live dispatch needs a separate explicit authorization and trusted
    integrations, none of which this pilot provides.
    """
    if type(source) is not OfflineSelectionSource or type(host) is not OfflineMockHost:
        raise DispatchRejected("offline source and scripted host are required")
    key = _required(request.get("invocation_key"), "invocation_key")
    if type(permit) is not OfflinePilotPermit or permit.invocation_key != key:
        raise DispatchRejected("separate offline pilot permit is missing")
    if now is None:
        now = datetime.now(timezone.utc)
    if now.tzinfo is None:
        raise DispatchRejected("now needs a timezone")
    record, route = source.verify(request, now)
    cwd, prompt = _required(request.get("cwd"), "cwd"), _required(request.get("prompt"), "prompt")
    if not Path(cwd).is_absolute() or len(prompt) > 16384:
        raise DispatchRejected("task packet is invalid or unbounded")
    surface = host.attest_tool_surface()
    if surface != {"kind": "scripted_offline", "mcp_servers": [], "mcp_tool_ids": []}:
        raise DispatchRejected("host tool surface is unknown or includes MCP")
    directory = _ledger(ledger_dir)
    key_digest = hashlib.sha256(key.encode()).hexdigest()
    try:
        _append_stage(directory, key_digest, 0, "reserved", invocation_key=key,
                      decision_id=record["decision_id"], preparation_digest=record["preparation_digest"],
                      requested_route=record["requested_route"], selected_route=record["selected_route"],
                      model=route["model"], effort=route["effort"], at=now.isoformat())
    except FileExistsError as error:
        raise DispatchRejected("invocation key already reserved; no retry") from error
    result: dict[str, Any] = {"status": "unknown_after_reservation",
                              "requested_route": record["requested_route"],
                              "selected_route": record["selected_route"],
                              "dispatched_configured": None, "observed_actual": None,
                              "turn_outcome": None, "trust_boundary": "offline_fixture_only"}
    stage = 1
    try:
        started = host.request("thread/start", {
            "cwd": cwd, "approvalPolicy": "never", "sandbox": "read-only", "ephemeral": True,
            "model": route["model"], "modelProvider": "openai",
            "reasoningEffort": route["effort"], "allowProviderModelFallback": False,
            "developerInstructions": "Complete only the supplied bounded task. Do not spawn agents or use MCP tools.",
        })
        thread_id = _required(started.get("thread", {}).get("id"), "thread id")
        _append_stage(directory, key_digest, stage, "thread_started", thread_id=thread_id)
        stage += 1
        metadata = _read_thread(host.request("thread/read", {"threadId": thread_id,
                                                              "includeTurns": False}),
                                thread_id, route["model"], route["effort"])
        result["dispatched_configured"] = {"thread_id": thread_id,
                                           "model": metadata["model"],
                                           "effort": metadata["reasoningEffort"],
                                           "fallback_allowed": False}
        _append_stage(directory, key_digest, stage, "metadata_checked", thread_id=thread_id,
                      configured=result["dispatched_configured"])
        stage += 1
        turn = host.request("turn/start", {"threadId": thread_id,
                                           "input": [{"type": "text", "text": prompt}],
                                           "model": route["model"], "effort": route["effort"]})
        turn_id = _required(turn.get("turn", {}).get("id"), "turn id")
        _append_stage(directory, key_digest, stage, "turn_started", thread_id=thread_id, turn_id=turn_id)
        stage += 1
        read = host.request("thread/read", {"threadId": thread_id, "includeTurns": True})
        metadata = _read_thread(read, thread_id, route["model"], route["effort"])
        turns = metadata.get("turns", [])
        matching = [item for item in turns if isinstance(item, dict) and item.get("id") == turn_id]
        if len(matching) > 1:
            raise RuntimeError("duplicate turn outcome in host read")
        result["turn_outcome"] = matching[0].get("status") if matching else None
        result["turn_id"] = turn_id
        result["status"] = "started" if matching else "started_outcome_pending"
        _append_stage(directory, key_digest, stage, "outcome_read", thread_id=thread_id,
                      turn_id=turn_id, turn_outcome=result["turn_outcome"])
    except Exception as error:
        result["error"] = f"{type(error).__name__}: {error}"
        try:
            _append_stage(directory, key_digest, stage, "unknown", error=result["error"])
        except Exception:
            result["ledger_error"] = "could not append unknown stage; reservation remains consumed"
    return result
