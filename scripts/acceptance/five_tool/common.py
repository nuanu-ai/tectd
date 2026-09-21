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
            "_meta": {"threadId": "00000000-0000-4000-8000-000000000099"},
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
