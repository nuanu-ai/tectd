"""One explicit S05 development case observed through an owned App Server.

The composition root owns/initializes the local stdio RPC process with MCP and
plugins disabled before construction. This module accepts that exact transport,
not imported receipts, caller booleans, JSON files or model self-description.
Python dependency wiring is a trust boundary, not protection against arbitrary
code in the owner process. The private offline test seam proves logic only.
Start configuration is not serving-model telemetry: observed_actual stays null.
This transport cannot attest historical agents.spawn_agent tasks.
"""

from __future__ import annotations

import hashlib
import json
import os
import stat
import threading
from dataclasses import asdict, dataclass, field
from pathlib import Path
from typing import Any

from scripts.codex_route_catalogue import (RouteSelection, one_off_catalogue,
                                          one_off_prompt, ONE_OFF_INVOCATION_KEY)


class ObservationRejected(ValueError):
    """No new turn may be started for this invalid or consumed intent."""


def _json(value: Any) -> str:
    return json.dumps(value, sort_keys=True, separators=(",", ":"),
                      ensure_ascii=False, allow_nan=False)


def _sha(value: str) -> str:
    return hashlib.sha256(value.encode("utf-8")).hexdigest()


def _exact(value: Any, name: str) -> str:
    if not isinstance(value, str) or not value or value != value.strip():
        raise ObservationRejected(f"{name} must be a nonempty exact string")
    return value


@dataclass(frozen=True)
class ExecutionIntent:
    selection: RouteSelection
    prompt: str
    invocation_key: str
    cwd: str
    requested: RouteSelection | None = None
    recommended: RouteSelection | None = None

    def __post_init__(self) -> None:
        if type(self.selection) is not RouteSelection:
            raise ObservationRejected("immutable catalogue selection required")
        _exact(self.invocation_key, "invocation key")
        _exact(self.cwd, "cwd")
        if not Path(self.cwd).is_absolute():
            raise ObservationRejected("cwd must be absolute")
        if not isinstance(self.prompt, str) or not self.prompt.strip() or len(self.prompt.encode()) > 16384:
            raise ObservationRejected("prompt must be nonempty and bounded")
        if self.selection.task_input_digest != self.prompt_digest:
            raise ObservationRejected("selected task input digest differs from exact prompt")
        if self.selection.catalogue_digest == one_off_catalogue().digest and (
                self.invocation_key != ONE_OFF_INVOCATION_KEY or self.prompt != one_off_prompt() or
                self.requested is not None or self.recommended is not None):
            raise ObservationRejected("one-off intent must match the fixed approval scope")
        for stage in (self.requested, self.recommended):
            if stage is not None and (type(stage) is not RouteSelection or
                                     stage.task_input_digest != self.prompt_digest or
                                     stage.catalogue_digest != self.selection.catalogue_digest):
                raise ObservationRejected("requested/recommended stage binding differs")

    @property
    def prompt_digest(self) -> str:
        return _sha(self.prompt)

    @property
    def digest(self) -> str:
        return _sha(_json({"selection": asdict(self.selection), "prompt_sha256": self.prompt_digest,
                          "invocation_key": self.invocation_key, "cwd": self.cwd,
                          "requested": None if self.requested is None else asdict(self.requested),
                          "recommended": None if self.recommended is None else asdict(self.recommended)}))


@dataclass(frozen=True)
class DispatchedConfiguration:
    model: str
    provider: str
    effort: str


@dataclass(frozen=True)
class AppServerReceipt:
    intent_digest: str
    selection: RouteSelection
    requested: RouteSelection | None
    recommended: RouteSelection | None
    prompt_digest: str
    invocation_key: str
    evidence_kind: str
    status: str
    thread_id: str | None
    turn_id: str | None
    dispatched_configured: DispatchedConfiguration | None
    terminal_outcome: str | None
    failure: str | None
    events_json: str
    notification_capture_complete: bool
    host_kind: str = field(default="APP_SERVER", init=False)
    observed_actual: None = field(default=None, init=False)


class _Ledger:
    """One-use records are replay fences, never authentication of stored JSON."""

    def __init__(self, directory: Path):
        if not directory.is_absolute():
            raise ObservationRejected("ledger directory must be absolute")
        try:
            directory.mkdir(mode=0o700)
        except FileExistsError:
            pass
        self.fd = os.open(directory, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
        info = os.fstat(self.fd)
        if not stat.S_ISDIR(info.st_mode) or info.st_uid != os.getuid() or info.st_mode & 0o077:
            self.close()
            raise ObservationRejected("ledger must be an owner-only directory")

    def write(self, filename: str, record: dict[str, Any]) -> None:
        fd = os.open(filename, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW,
                     0o600, dir_fd=self.fd)
        with os.fdopen(fd, "w", encoding="utf-8") as stream:
            stream.write(_json(record) + "\n")
            stream.flush()
            os.fsync(stream.fileno())
        os.fsync(self.fd)

    def close(self) -> None:
        os.close(self.fd)


class AppServerObserver:
    """Trusted composition-root use case; no effects until explicit run_once.

    Retain one object to reuse its immutable terminal observation. An existing
    durable reservation always blocks a new object; disk contents are not proof.
    The composition root must initialize RPC first. No automatic retries occur.
    """

    def __init__(self, rpc: Any, *, ledger_dir: Path):
        from scripts.codex_app_server_rpc import OwnedAppServerRpc
        if type(rpc) is not OwnedAppServerRpc:
            raise ObservationRejected("exact owned App Server stdio transport required")
        self._configure(rpc, ledger_dir, "owned_stdio")

    def _configure(self, rpc: Any, ledger_dir: Path, evidence_kind: str) -> None:
        # Private offline tests deliberately bypass public composition. They do
        # not authenticate a host and are labelled offline_fixture in receipts.
        self._rpc = rpc
        self._ledger_dir = ledger_dir
        self._evidence_kind = evidence_kind
        self._lock = threading.Lock()
        self._terminal: dict[str, tuple[str, AppServerReceipt]] = {}

    def _inventory(self, method: str, params: dict[str, Any], events: list[Any]) -> list[Any]:
        rows, seen = [], set()
        for _ in range(16):
            result = self._rpc.request(method, params, timeout=30)
            if method == "mcpServerStatus/list":
                # Inventory may carry descriptions, URLs, and server-controlled
                # payloads. Retain only shape metadata, even on rejected rows.
                data = result.get("data") if isinstance(result, dict) else None
                summaries = []
                if isinstance(data, list):
                    for row in data:
                        if not isinstance(row, dict):
                            summaries.append({"type": type(row).__name__})
                            continue
                        name = row.get("name")
                        summary = {"name_sha256": _sha(name) if isinstance(name, str) else None,
                                   "name_type": type(name).__name__,
                                   "authStatus": "unsupported" if row.get("authStatus") == "unsupported" else None,
                                   "serverInfoIsNull": row.get("serverInfo") is None}
                        for key in ("tools", "resources", "resourceTemplates"):
                            value = row.get(key)
                            summary[key] = {"type": type(value).__name__,
                                            "count": len(value) if isinstance(value, (dict, list)) else None}
                        summaries.append(summary)
                events.append({"method": method, "inventory_shape": {
                    "data_type": type(data).__name__, "entries": summaries}})
            else:
                events.append({"method": method, "response": result})
            if not isinstance(result, dict) or not isinstance(result.get("data"), list):
                raise ObservationRejected(f"missing {method} host inventory")
            rows.extend(result["data"])
            cursor = result.get("nextCursor")
            if cursor is None:
                return rows
            if not isinstance(cursor, str) or not cursor or cursor in seen:
                raise ObservationRejected("invalid inventory pagination")
            seen.add(cursor)
            params = dict(params, cursor=cursor)
        raise ObservationRejected("host inventory exceeds bounded pagination")

    def _disabled_surface(self, events: list[Any], cwd: str, thread_id: str | None = None) -> None:
        # config/read returns the effective host map, not caller TOML or launch
        # arguments. MCP snake-case shape is confirmed against CLI 0.146.1.
        # Feature/plugin shape remains unverified live: missing shape fails closed.
        response = self._rpc.request("config/read", {"includeLayers": False, "cwd": cwd}, timeout=30)
        config = response.get("config") if isinstance(response, dict) else None
        if not isinstance(config, dict):
            raise ObservationRejected("missing effective host config")
        servers, features, plugins = (config.get(key) for key in ("mcp_servers", "features", "plugins"))
        flags = ("plugins", "remote_plugin", "apps", "shell_tool", "unified_exec", "multi_agent", "multi_agent_v2")
        # Persist only safe metadata; raw config can contain credentials.
        def summarize_identifier_map(value: Any) -> list[dict[str, Any]] | None:
            if not isinstance(value, dict):
                return None
            return [{"identifier_sha256": _sha(name) if isinstance(name, str) else None,
                     "identifier_type": type(name).__name__,
                     "entry_type": type(entry).__name__,
                     "enabled": entry.get("enabled") if isinstance(entry, dict) and
                     type(entry.get("enabled")) is bool else None}
                    for name, entry in value.items()]

        safe_servers = summarize_identifier_map(servers)
        safe_plugins = summarize_identifier_map(plugins)
        safe_features = ({flag: features.get(flag) if type(features.get(flag)) is bool else None
                          for flag in flags} if isinstance(features, dict) else None)
        web_search = config.get("web_search")
        safe_web_search = web_search if isinstance(web_search, str) and web_search in {
            "disabled", "cached", "live"} else None
        events.append({"method": "config/read", "effective_surface": {
            "mcp_enabled": safe_servers, "plugin_enabled": safe_plugins,
            "features": safe_features, "web_search": safe_web_search}})
        if not isinstance(servers, dict) or safe_servers is None or len(safe_servers) != len(servers):
            raise ObservationRejected("effective MCP map shape is unknown")
        if any(not isinstance(name, str) or not name or not isinstance(entry, dict) or
               type(entry.get("enabled")) is not bool or entry["enabled"] is not False
               for name, entry in servers.items()):
            raise ObservationRejected("every effective MCP server must be explicitly disabled")
        if not isinstance(features, dict) or any(features.get(flag) is not False for flag in flags):
            raise ObservationRejected("effective plugin/app/tool feature disables are missing")
        if not isinstance(plugins, dict) or safe_plugins is None or len(safe_plugins) != len(plugins) or any(
                not isinstance(name, str) or not name or not isinstance(entry, dict) or
                type(entry.get("enabled")) is not bool or entry["enabled"] is not False
                for name, entry in plugins.items()):
            raise ObservationRejected("effective plugin map must contain only disabled plugins")
        if config.get("web_search") != "disabled":
            raise ObservationRejected("effective web search must be disabled")
        params: dict[str, Any] = {"limit": 100, "detail": "full"}
        if thread_id is not None:
            params["threadId"] = thread_id
        rows = self._inventory("mcpServerStatus/list", params, events)
        names: set[str] = set()
        for row in rows:
            if not isinstance(row, dict) or not isinstance(row.get("name"), str):
                raise ObservationRejected("MCP status name is missing")
            name = row["name"]
            if name in names or name not in servers:
                raise ObservationRejected("MCP status name is duplicated or absent from effective disabled map")
            names.add(name)
            if (row.get("authStatus") != "unsupported" or row.get("tools") != {} or
                    row.get("resources") != [] or row.get("resourceTemplates") != [] or
                    row.get("serverInfo") is not None):
                raise ObservationRejected("disabled MCP metadata has active or unknown inventory")
        if names != set(servers):
            raise ObservationRejected("effective configured MCP entries are not fully reported")

    @staticmethod
    def _turn(read: Any, intent: ExecutionIntent, thread_id: str, turn_id: str) -> dict[str, Any]:
        thread = read.get("thread") if isinstance(read, dict) else None
        if not isinstance(thread, dict) or thread.get("id") != thread_id:
            raise ObservationRejected("thread/read identity mismatch")
        if thread.get("cwd") != intent.cwd or thread.get("modelProvider") != "openai":
            raise ObservationRejected("thread/read context mismatch")
        turns = thread.get("turns")
        if not isinstance(turns, list) or len(turns) != 1 or not isinstance(turns[0], dict):
            raise ObservationRejected("fresh thread must have exactly one observed turn")
        turn = turns[0]
        if turn.get("id") != turn_id or turn.get("status") != "completed" or turn.get("error") is not None:
            raise ObservationRejected("thread/read terminal turn mismatch")
        if turn.get("itemsView", "full") != "full" or not isinstance(turn.get("items"), list):
            raise ObservationRejected("full persisted turn items required")
        items = turn["items"]
        if any(not isinstance(item, dict) or item.get("type") not in
               {"userMessage", "agentMessage", "reasoning"} for item in items):
            raise ObservationRejected("tool use or unknown item in bounded no-tool task")
        users = [item for item in items if item["type"] == "userMessage"]
        if len(users) != 1 or users[0].get("clientId") not in (None, intent.invocation_key):
            raise ObservationRejected("exact user message identity required")
        content = users[0].get("content")
        if (not isinstance(content, list) or len(content) != 1 or not isinstance(content[0], dict) or
                content[0].get("type") != "text" or content[0].get("text") != intent.prompt or
                content[0].get("text_elements", []) != []):
            raise ObservationRejected("persisted prompt differs from exact intent")
        if not any(item["type"] == "agentMessage" and isinstance(item.get("text"), str)
                   and item["text"].strip() for item in items):
            raise ObservationRejected("missing completed agent output")
        return turn

    def run_once(self, intent: ExecutionIntent, *, completion_timeout: float = 60) -> AppServerReceipt:
        if type(intent) is not ExecutionIntent:
            raise ObservationRejected("immutable execution intent required")
        maximum_timeout = 180 if intent.selection.catalogue_digest == one_off_catalogue().digest else 60
        if isinstance(completion_timeout, bool) or not isinstance(completion_timeout, (int, float)) or not 0 < completion_timeout <= maximum_timeout:
            raise ObservationRejected(f"completion timeout must be within {maximum_timeout} seconds")
        key_digest = _sha(intent.invocation_key)
        with self._lock:
            previous = self._terminal.get(key_digest)
            if previous is not None:
                if previous[0] != intent.digest:
                    raise ObservationRejected("invocation key conflicts with previous intent")
                return previous[1]
            return self._run_reserved(intent, key_digest, completion_timeout)

    def _run_reserved(self, intent: ExecutionIntent, key_digest: str, timeout: float) -> AppServerReceipt:
        ledger = _Ledger(self._ledger_dir)
        try:
            ledger.write(key_digest + ".00-reserved.json", {"stage": "reserved",
                         "intent_digest": intent.digest, "prompt_digest": intent.prompt_digest,
                         "invocation_key": intent.invocation_key, "selection": asdict(intent.selection)})
        except FileExistsError as error:
            ledger.close()
            raise ObservationRejected("invocation key already reserved; no new start or retry") from error
        except Exception:
            ledger.close()
            raise
        events: list[Any] = []
        thread_id = turn_id = outcome = failure = None
        configured = None
        status = "unknown_after_reservation"
        completion_notification: dict[str, Any] | None = None
        capture_snapshot_returned = False
        notification_capture_complete = False
        try:
            selection = intent.selection
            models = self._inventory("model/list", {"includeHidden": True, "limit": 100}, events)
            matches = [model for model in models if isinstance(model, dict) and model.get("model") == selection.model]
            if len(matches) != 1 or not isinstance(matches[0].get("supportedReasoningEfforts"), list):
                raise ObservationRejected("selected model not uniquely advertised by host")
            efforts = matches[0]["supportedReasoningEfforts"]
            if not any(isinstance(option, dict) and option.get("reasoningEffort") == selection.effort
                       for option in efforts):
                raise ObservationRejected("selected effort not advertised by host")
            self._disabled_surface(events, intent.cwd)
            params = {"model": selection.model, "modelProvider": "openai", "cwd": intent.cwd,
                      "approvalPolicy": "never", "sandbox": "read-only", "ephemeral": True,
                      "allowProviderModelFallback": False,
                      "config": {"model_reasoning_effort": selection.effort},
                      "developerInstructions": "Complete only the exact supplied bounded task. Do not use tools, MCP, plugins or agents."}
            events.append({"method": "thread/start", "request": params})
            started = self._rpc.request("thread/start", params, timeout=30)
            events.append({"method": "thread/start", "response": started})
            if not isinstance(started, dict) or not isinstance(started.get("thread"), dict):
                raise ObservationRejected("missing thread/start host response")
            thread_id = _exact(started["thread"].get("id"), "thread id")
            if (started.get("model"), started.get("modelProvider"), started.get("reasoningEffort")) != (
                    selection.model, "openai", selection.effort):
                raise ObservationRejected("thread/start configured model/provider/effort mismatch")
            if (started.get("approvalPolicy") != "never" or started.get("cwd") != intent.cwd or
                    not isinstance(started.get("sandbox"), dict) or started["sandbox"].get("type") != "readOnly"):
                raise ObservationRejected("thread/start execution boundary mismatch")
            configured = DispatchedConfiguration(started["model"], started["modelProvider"], started["reasoningEffort"])
            self._disabled_surface(events, intent.cwd, thread_id)
            params = {"threadId": thread_id, "model": selection.model, "effort": selection.effort,
                      "clientUserMessageId": intent.invocation_key,
                      "input": [{"type": "text", "text": intent.prompt, "text_elements": []}]}
            ledger.write(key_digest + ".01-before-turn.json", {"stage": "before_turn", "thread_id": thread_id,
                         "intent_digest": intent.digest, "request": params, "events": events})
            events.append({"method": "turn/start", "request": params})
            accepted = self._rpc.request("turn/start", params, timeout=30)
            events.append({"method": "turn/start", "response": accepted})
            turn = accepted.get("turn") if isinstance(accepted, dict) else None
            if not isinstance(turn, dict) or turn.get("status") not in {"inProgress", "completed", "failed", "interrupted"}:
                raise ObservationRejected("missing accepted turn/start host data")
            turn_id = _exact(turn.get("id"), "turn id")
            completed = self._rpc.wait_notification("turn/completed", lambda value:
                isinstance(value.get("params"), dict) and value["params"].get("threadId") == thread_id
                and isinstance(value["params"].get("turn"), dict)
                and value["params"]["turn"].get("id") == turn_id, timeout=timeout)
            completion_notification = completed
            completion_params = completed.get("params") if isinstance(completed, dict) else None
            if (not isinstance(completion_params, dict) or completion_params.get("threadId") != thread_id or
                    not isinstance(completion_params.get("turn"), dict) or completion_params["turn"].get("id") != turn_id):
                raise ObservationRejected("completion host identity mismatch")
            outcome = completion_params["turn"].get("status")
            read = self._rpc.request("thread/read", {"threadId": thread_id, "includeTurns": True}, timeout=30)
            events.append({"method": "thread/read", "response": read})
            if outcome != "completed":
                raise ObservationRejected("host task did not complete successfully")
            self._turn(read, intent, thread_id, turn_id)

            try:
                notifications = self._rpc.seal_notifications(timeout=30)
            except Exception as error:
                raise ObservationRejected(
                    f"complete notification stream capture failed ({type(error).__name__})"
                ) from None
            capture_snapshot_returned = True
            notification_capture_complete = self._evidence_kind == "owned_stdio"
            if not isinstance(notifications, list) or any(not isinstance(value, dict) for value in notifications):
                raise ObservationRejected("complete notification stream snapshot is malformed")
            events.extend({"notification": value} for value in notifications)
            matching_completions = [
                value for value in notifications
                if value.get("method") == "turn/completed"
                and isinstance(value.get("params"), dict)
                and value["params"].get("threadId") == thread_id
                and isinstance(value["params"].get("turn"), dict)
                and value["params"]["turn"].get("id") == turn_id
            ]
            if len(matching_completions) != 1 or matching_completions[0] != completion_notification:
                raise ObservationRejected("complete notification stream lacks the unique observed turn completion")
            status = "completed_configured_route"
        except ObservationRejected as error:
            status, failure = "configured_route_rejected", str(error)
        except Exception as error:
            failure = f"host transport failure ({type(error).__name__})"
        finally:
            if completion_notification is not None and not capture_snapshot_returned:
                events.append({"method": "turn/completed", "notification": completion_notification})
            try:
                if not capture_snapshot_returned:
                    remaining = self._rpc.take_notifications()
                    events.extend({"notification": value} for value in remaining)
            except Exception as error:
                events.append({"notification_read_error": type(error).__name__})
                if status == "completed_configured_route":
                    status, failure = "configured_route_rejected", "host notification evidence unavailable"
            for event in events:
                notification = event.get("notification")
                if not isinstance(notification, dict):
                    continue
                params = notification.get("params", {})
                if not isinstance(params, dict):
                    continue
                if params.get("threadId") != thread_id or params.get("turnId") != turn_id:
                    continue
                item = params.get("item")
                if notification.get("method") == "model/rerouted":
                    status, failure = "configured_route_rejected", "host reported model reroute"
                elif isinstance(item, dict) and item.get("type") not in {"userMessage", "agentMessage", "reasoning"}:
                    status, failure = "configured_route_rejected", "host notification reports tool use or unknown item"
            receipt = AppServerReceipt(intent.digest, intent.selection, intent.requested, intent.recommended,
                        intent.prompt_digest, intent.invocation_key, self._evidence_kind, status,
                        thread_id, turn_id, configured, outcome, failure, _json(events),
                        notification_capture_complete)
            try:
                ledger.write(key_digest + ".02-observation.json", {"stage": "observation", "receipt": asdict(receipt)})
            finally:
                ledger.close()
        if outcome in {"completed", "failed", "interrupted"}:
            self._terminal[key_digest] = (intent.digest, receipt)
        return receipt
