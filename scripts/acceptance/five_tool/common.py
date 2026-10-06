#!/usr/bin/env python3
"""Small JSON-RPC and proof helpers for the isolated five-tool acceptance."""

from __future__ import annotations

import hashlib
import json
import os
import pathlib
import select
import subprocess
import time
from typing import Any


class RpcError(RuntimeError):
    def __init__(self, error: dict[str, Any]):
        super().__init__(json.dumps(error, sort_keys=True))
        self.error = error


def sha256_file(path: pathlib.Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def sha256_json(value: Any) -> str:
    encoded = json.dumps(value, sort_keys=True, separators=(",", ":")).encode()
    return hashlib.sha256(encoded).hexdigest()


class Proof:
    def __init__(self, path: pathlib.Path, initial: dict[str, Any]):
        self.path = path
        self.data = initial
        self.persist()

    def persist(self) -> None:
        self.path.parent.mkdir(parents=True, exist_ok=True)
        temporary = self.path.with_suffix(self.path.suffix + ".tmp")
        temporary.write_text(json.dumps(self.data, indent=2, ensure_ascii=False) + "\n")
        temporary.replace(self.path)

    def check(self, name: str, passed: bool, detail: Any = None) -> None:
        item = {"name": name, "passed": bool(passed)}
        if detail is not None:
            item["detail"] = detail
        self.data.setdefault("checks", []).append(item)
        self.persist()
        if not passed:
            raise AssertionError(name)


class Rpc:
    def __init__(self, command: list[str], env: dict[str, str], cwd: pathlib.Path):
        self.process = subprocess.Popen(
            command,
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            env=env,
            cwd=cwd,
        )
        self.sequence = 0
        self.buffer = b""
        self.notifications: list[dict[str, Any]] = []

    def _read(self, timeout: float = 30) -> dict[str, Any]:
        deadline = time.monotonic() + timeout
        assert self.process.stdout is not None
        while b"\n" not in self.buffer:
            remaining = deadline - time.monotonic()
            if remaining <= 0 or not select.select([self.process.stdout], [], [], remaining)[0]:
                raise TimeoutError("Codex app-server JSON-RPC timed out")
            block = os.read(self.process.stdout.fileno(), 65536)
            if not block:
                stderr = b""
                if self.process.stderr is not None:
                    stderr = self.process.stderr.read(8192)
                raise RuntimeError(f"Codex app-server exited: {stderr.decode(errors='replace')}")
            self.buffer += block
        line, self.buffer = self.buffer.split(b"\n", 1)
        return json.loads(line)

    def request(self, method: str, params: dict[str, Any], timeout: float = 30) -> Any:
        self.sequence += 1
        request_id = self.sequence
        assert self.process.stdin is not None
        message = {"id": request_id, "method": method, "params": params}
        self.process.stdin.write((json.dumps(message) + "\n").encode())
        self.process.stdin.flush()
        while True:
            message = self._read(timeout)
            if message.get("id") == request_id and "method" not in message:
                if "error" in message:
                    raise RpcError(message["error"])
                return message["result"]
            self.notifications.append(message)

    def notify(self, method: str, params: dict[str, Any]) -> None:
        assert self.process.stdin is not None
        self.process.stdin.write((json.dumps({"method": method, "params": params}) + "\n").encode())
        self.process.stdin.flush()

    def close(self) -> None:
        if self.process.poll() is None:
            self.process.terminate()
            try:
                self.process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                self.process.kill()
                self.process.wait(timeout=5)


def initialize(app: Rpc, client_name: str) -> None:
    app.request(
        "initialize",
        {"clientInfo": {"name": client_name, "version": "1.0"}, "capabilities": {"experimentalApi": True}},
    )
    app.notify("initialized", {})


def start_thread(app: Rpc, cwd: pathlib.Path, developer: str) -> str:
    result = app.request(
        "thread/start",
        {
            "cwd": str(cwd),
            "approvalPolicy": "never",
            "sandbox": "read-only",
            "ephemeral": True,
            "model": "gpt-5.6-sol",
            "modelProvider": "openai",
            "allowProviderModelFallback": False,
            "developerInstructions": developer,
        },
    )
    return result["thread"]["id"]


def raw_tool_result(app: Rpc, thread_id: str, tool: str, arguments: dict[str, Any]) -> dict[str, Any]:
    return app.request(
        "mcpServer/tool/call",
        {
            "threadId": thread_id,
            "server": "tectd",
            "tool": tool,
            "arguments": arguments,
        },
    )


def tool_result(app: Rpc, thread_id: str, tool: str, arguments: dict[str, Any]) -> tuple[dict[str, Any], bool]:
    result = raw_tool_result(app, thread_id, tool, arguments)
    content = result.get("content", [])
    if len(content) < 2 or content[1].get("type") != "text":
        raise AssertionError("tool result lacks the canonical JSON text block")
    return json.loads(content[1]["text"]), bool(result.get("isError"))


def command_overrides(package: pathlib.Path, launcher: pathlib.Path, socket: pathlib.Path, host_config: pathlib.Path, workspace: str) -> list[str]:
    settings: dict[str, Any] = {
        "mcp_servers.tectd.command": str(launcher),
        "mcp_servers.tectd.args": [],
        "mcp_servers.tectd.cwd": str(package),
        "mcp_servers.tectd.enabled": True,
        "mcp_servers.tectd.startup_timeout_sec": 20,
        "mcp_servers.tectd.env.TECT_SOCKET": str(socket),
        "mcp_servers.tectd.env.TECT_HOST_CONFIG": str(host_config),
        "mcp_servers.tectd.env.TECT_WORKSPACE_KEY": workspace,
        "analytics.enabled": False,
    }
    for tool in ["get_state", "query", "command", "execute", "help"]:
        settings[f"mcp_servers.tectd.tools.{tool}.approval_mode"] = "approve"
    result: list[str] = []
    for key, value in settings.items():
        result.extend(["-c", f"{key}={json.dumps(value)}"])
    return result


def read_pipeline_json(call, tool: str, route: str, params: dict[str, Any]) -> dict[str, Any]:
    """Reassemble one pinned JSON representation above the MCP payload decoder.

    This read allocates no receipt. Its byte hash proves retrieved content, never
    workflow consumption or the semantic truth of an acceptance fixture.
    """
    import copy
    initial = copy.deepcopy(params)
    if initial.get("offset_bytes", 0) != 0:
        raise AssertionError("complete JSON read must begin at byte zero")
    selector_keys = {"run_id", "definition_digest", "phase_id", "run_revision", "section",
                     "output_id", "digest", "instruction_id", "version", "refresh"}
    pins = {key: initial[key] for key in selector_keys if key in initial}
    payload, failed = call(tool, {"route": route, "params": initial})
    if failed:
        raise AssertionError(f"{route} pinned read failed: {payload.get('error', {}).get('code')}")
    if payload.get("kind") != "fragment":
        if any(key in initial for key in ("offset_bytes", "limit_bytes", "representation_digest")):
            raise AssertionError("explicit byte request did not return a fragment")
        _check_pipeline_read_pins(payload, pins)
        return payload
    source = copy.deepcopy(payload.get("source"))
    if not isinstance(source, dict) or any(source.get(key) != value for key, value in pins.items()):
        raise AssertionError("fragment source does not match requested pins")
    representation = payload.get("representation_digest")
    if not isinstance(representation, str) or len(representation) != 64 or any(c not in "0123456789abcdef" for c in representation):
        raise AssertionError("invalid representation digest")
    if initial.get("representation_digest") is not None and initial["representation_digest"] != representation:
        raise AssertionError("initial representation pin changed")
    total = payload.get("total_bytes")
    if type(total) is not int or total < 1:
        raise AssertionError("invalid complete representation size")
    chunks, offset = [], 0
    request = initial
    while True:
        if (payload.get("kind"), payload.get("format"), payload.get("encoding")) != ("fragment", "json", "utf-8"):
            raise AssertionError("fragment format changed")
        if payload.get("source") != source or payload.get("representation_digest") != representation or payload.get("total_bytes") != total:
            raise AssertionError("fragment source, revision, size or digest changed")
        text = payload.get("text")
        if not isinstance(text, str):
            raise AssertionError("fragment text missing")
        block = text.encode("utf-8", errors="strict")
        returned = payload.get("returned_bytes")
        if (type(payload.get("offset_bytes")) is not int or payload["offset_bytes"] != offset
                or type(returned) is not int or returned != len(block)
                or returned > min(4096, request.get("limit_bytes", 4096))
                or offset + returned > total):
            raise AssertionError("fragment byte offset, length or limit invalid")
        chunks.append(block)
        end = offset + returned
        if "next_offset_bytes" not in payload:
            raise AssertionError("fragment EOF marker missing")
        next_offset = payload["next_offset_bytes"]
        actions = payload.get("actions", [])
        if next_offset is None:
            if end != total:
                raise AssertionError("premature fragment EOF")
            break
        if type(next_offset) is not int or next_offset != end or end <= offset or end >= total:
            raise AssertionError("fragment continuation made no valid progress")
        if len(actions) != 1:
            raise AssertionError("intermediate fragment must expose only its byte continuation")
        continuation = actions[0]
        arguments = continuation.get("arguments", {})
        next_params = arguments.get("params")
        if continuation.get("tool") != tool or arguments.get("route") != route or not isinstance(next_params, dict):
            raise AssertionError("fragment continuation route changed")
        expected = {**initial, "representation_digest": representation,
                    "limit_bytes": initial.get("limit_bytes", 4096), "offset_bytes": end}
        # The actual context-query DTO serializes its ordinary false default.
        if route == "slice.pipeline.context" and "refresh" not in expected:
            expected["refresh"] = False
        actual = dict(next_params)
        if route == "slice.pipeline.context" and "refresh" not in actual and expected.get("refresh") is False:
            expected.pop("refresh")
        if actual != expected:
            raise AssertionError("fragment continuation changed pinned parameters")
        request = copy.deepcopy(next_params)  # Follow the exact returned wire call.
        payload, failed = call(tool, copy.deepcopy(arguments))
        if failed:
            raise AssertionError("pinned fragment continuation refused")
        offset = end
    original = b"".join(chunks)
    if len(original) != total or hashlib.sha256(original).hexdigest() != representation:
        raise AssertionError("complete original UTF-8 representation digest mismatch")
    value = json.loads(original.decode("utf-8"))
    if not isinstance(value, dict):
        raise AssertionError("pinned JSON payload is not an object")
    _check_pipeline_read_pins(value, pins)
    value["actions"] = actions  # Collection cursors become available only after verified EOF.
    return value


def _check_pipeline_read_pins(payload: dict[str, Any], pins: dict[str, Any]) -> None:
    for key, expected in pins.items():
        if key == "refresh":  # Instruction refresh requests a body; it is not a returned pin.
            continue
        actual = payload.get(key)
        if key == "phase_id" and "phase" in payload:
            actual = payload["phase"].get("id")
        if key == "output_id":
            actual = payload.get("id")
        if key == "instruction_id" and "instruction" in payload:
            actual = payload["instruction"].get("id")
        if key in {"version", "digest"} and "instruction" in payload:
            actual = payload["instruction"].get(key)
        if actual != expected:
            raise AssertionError(f"pinned read changed {key}")


def hydrate_pipeline_payload(call, payload: dict[str, Any]) -> dict[str, Any]:
    """Resolve actual snapshot/phase/details destinations of compact lifecycle replies."""
    import copy
    result = copy.deepcopy(payload)
    contexts = [result] + [result.get(key) for key in ("created", "replay", "context")]
    for context in contexts:
        if not isinstance(context, dict) or context.get("delivery_scope") != "snapshot_reference":
            continue
        run = context["run"]
        def destination(view: str, **pins):
            for action in result.get("actions", []):
                args = action.get("arguments", {})
                candidate = args.get("params", {})
                if (action.get("tool") == "query" and args.get("route") == "slice.pipeline.context"
                        and candidate.get("view") == view and candidate.get("run_id") == run["id"]
                        and all(candidate.get(key) == value for key, value in pins.items())):
                    return copy.deepcopy(candidate)
            raise AssertionError(f"compact reply omitted exact pinned {view} destination")
        snapshot = read_pipeline_json(call, "query", "slice.pipeline.context",
            destination("snapshot", definition_digest=run["definition_digest"]))
        if (snapshot["definition"].get("digest") != run["definition_digest"]
                or snapshot["definition"].get("version") != run["definition_version"]
                or snapshot["definition"].get("kind") != run["definition_kind"]):
            raise AssertionError("retrieved definition identity changed")
        details = read_pipeline_json(call, "query", "slice.pipeline.context",
            destination("details", run_revision=run["revision"], section="all"))
        if not isinstance(details.get("data"), dict) or set(details["data"]) & {"run", "definition", "actions", "delivery_scope"}:
            raise AssertionError("details attempted to change lifecycle identity")
        context["definition"] = snapshot["definition"]
        context.update(details["data"])
        if context["run"].get("current_phase_id") is not None:
            phase_id = run["current_phase_id"]
            phase = read_pipeline_json(call, "query", "slice.pipeline.context",
                destination("phase_contract", definition_digest=run["definition_digest"], phase_id=phase_id))["phase"]
            stored = next((p for p in snapshot["definition"]["phases"] if p["id"] == phase_id), None)
            if phase != stored:
                raise AssertionError("phase contract differs from pinned complete snapshot")
        context["run"]["qualification_reason"] = details["data"].get("qualification_reason")
        context["retrieved_content"] = {"snapshot": True, "details_revision": run["revision"],
                                        "phase_contract": run.get("current_phase_id")}
        if "result_reference" in result:
            reference = result["result_reference"].get("result_id")
            terminal = context.get("result")
            if reference is not None and (not terminal or terminal.get("id") != reference):
                raise AssertionError("result history differs from returned result reference")
            result["result"] = terminal
    return result
