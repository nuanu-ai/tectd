"""Strict checks for the installed CLI's public app-server protocol."""
from __future__ import annotations

import json
import time
import uuid

RESPONSE_FOOTER = 'Follow the rules from workspace.open or help {"text":"response-rules"}. Required checks, approvals and authority still apply. Dependencies alone grant no permission or automatic resumption. Claim monitoring or continuation only when real.'

PUBLIC_TOOLS = {"get_state", "help", "query", "command", "execute"}


def parse_delegation_features(output: str, allow_one_child: bool) -> dict[str, bool]:
    states = {}
    for line in output.splitlines():
        row = line.split()
        if not row or row[0] not in {"multi_agent", "multi_agent_v2"}:
            continue
        if row[0] in states or len(row) < 3 or row[-1] not in {"true", "false"}:
            raise AssertionError("malformed or duplicate delegation feature")
        states[row[0]] = row[-1] == "true"
    if states != {"multi_agent": allow_one_child, "multi_agent_v2": False}:
        raise AssertionError("owned app-server delegation features do not match the requested test mode")
    return states


def wait_mcp_ready(app, thread_id: str, timeout: float = 20) -> dict:
    deadline = time.monotonic() + timeout
    position = 0
    while True:
        while position < len(app.notifications):
            event = app.notifications[position]
            position += 1
            if event.get("method") != "mcpServer/startupStatus/updated":
                continue
            params = event.get("params", {})
            if params.get("name") != "tectd" or params.get("threadId") != thread_id:
                continue
            state = params.get("status")
            if state == "ready":
                if params.get("error") or params.get("failureReason"):
                    raise AssertionError("MCP ready notification contains failure")
                return event
            if state != "starting":
                raise AssertionError("MCP startup failed, cancelled or malformed")
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            raise TimeoutError("owned MCP ready notification timed out")
        app.notifications.append(app._read(remaining))


def validate_catalog(server: dict) -> None:
    if "runtimeStatus" in server and server["runtimeStatus"] != "connected":
        raise AssertionError("reported MCP runtime is not connected")
    tools = server.get("tools")
    if not isinstance(tools, dict) or set(tools) != PUBLIC_TOOLS:
        raise AssertionError("native MCP status must discover exactly five public tools")
    if any(not isinstance(value, dict) for value in tools.values()):
        raise AssertionError("malformed MCP tool definition")


def validate_get_state(response: dict, thread_id: str) -> dict:
    try:
        parsed_id = uuid.UUID(thread_id)
        if str(parsed_id) != thread_id or parsed_id.int == 0:
            raise ValueError("invalid native ID")
        if "structuredContent" in response:
            raise ValueError("noncanonical structured result")
        content = response["content"]
        if response.get("isError", False) is not False or not isinstance(content, list) or len(content) != 3:
            raise ValueError("invalid result")
        if any(item.get("type") != "text" or not isinstance(item.get("text"), str) for item in content):
            raise ValueError("invalid content")
        intro = content[0]["text"]
        if not intro or len(intro.encode("utf-8")) > 2_000 or content[2]["text"] != RESPONSE_FOOTER:
            raise ValueError("missing canonical introduction or rules")
        payload = json.loads(content[1]["text"])
        if not isinstance(payload, dict) or payload.get("status") not in {"uninitialized", "ready"} or payload.get("error"):
            raise ValueError("invalid state")
        session = payload.get("session")
        if session is not None and session.get("native_session_id") != thread_id:
            raise ValueError("native identity mismatch")
        return payload
    except (KeyError, TypeError, ValueError, AttributeError) as error:
        raise AssertionError("malformed or misbound canonical get_state result") from error
