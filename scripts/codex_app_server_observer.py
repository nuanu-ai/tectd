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
import time
from dataclasses import asdict, dataclass, field
from pathlib import Path
from typing import Any

from scripts.codex_route_catalogue import (RouteSelection, one_off_catalogue,
    one_off_prompt, ONE_OFF_INVOCATION_KEY, persisted_catalogue, persisted_prompt,
    PERSISTED_INVOCATION_KEY)
from scripts.codex_app_server_rpc import AppServerRpcError


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
    ephemeral_thread: bool = True

    def __post_init__(self) -> None:
        if type(self.selection) is not RouteSelection:
            raise ObservationRejected("immutable catalogue selection required")
        if type(self.ephemeral_thread) is not bool:
            raise ObservationRejected("ephemeral_thread must be a strict bool")
        persisted = self.selection.catalogue_digest == persisted_catalogue().digest
        if persisted and (self.ephemeral_thread is not False or
                self.invocation_key != PERSISTED_INVOCATION_KEY or self.prompt != persisted_prompt() or
                self.requested is not None or self.recommended is not None):
            raise ObservationRejected("persisted intent must match the fixed approval scope")
        if not persisted and self.ephemeral_thread is not True:
            raise ObservationRejected("only the fixed persisted case may save a thread")
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
                          "ephemeral_thread": self.ephemeral_thread,
                          "requested": None if self.requested is None else asdict(self.requested),
                          "recommended": None if self.recommended is None else asdict(self.recommended)}))


@dataclass(frozen=True)
class CallerRouteSelection:
    """Configuration projected from a privately validated current source result.

    This value alone is neither catalogue authority nor dispatch permission.
    """

    route_id: str
    provider: str
    model: str
    effort: str

    def __post_init__(self) -> None:
        for name in ("route_id", "provider", "model", "effort"):
            _exact(getattr(self, name), name)


_CALLER_INTENT_SEAL = object()
_CALLER_SOURCE_TICKET_SEAL = object()


@dataclass(frozen=True, init=False)
class _CallerSourceTicket:
    """Opaque private bridge-issued provenance, never parsed from source JSON.

    Bound to one installed bridge/observer. This gates ordinary API use, not
    arbitrary Python code in the trusted owner composition process.
    """

    observer: Any
    issuer: Any
    selection: CallerRouteSelection
    requested: CallerRouteSelection | None
    recommended: CallerRouteSelection | None
    invocation_key: str
    input_sha256: str
    source_binding_json: str
    source_binding_digest: str
    _seal: Any = field(repr=False)

    def __init__(self, *args: Any, **kwargs: Any):
        raise ObservationRejected("current-source ticket requires private bridge issuance")


def _mint_caller_source_ticket(*, observer: Any, issuer: Any, selection: CallerRouteSelection,
        requested: CallerRouteSelection | None, recommended: CallerRouteSelection | None,
        invocation_key: str, input_sha256: str, source_binding_json: str,
        source_binding_digest: str) -> _CallerSourceTicket:
    """Private bridge call only, after its trusted-source and closed-wire checks."""
    if issuer is None or type(observer) is not AppServerObserver or getattr(observer, "_caller_source_issuer", None) is not issuer:
        raise ObservationRejected("ticket issuer differs from the installed caller composition")
    if type(selection) is not CallerRouteSelection or any(value is not None and
            type(value) is not CallerRouteSelection for value in (requested, recommended)):
        raise ObservationRejected("immutable caller route configurations required")
    if _sha(source_binding_json) != source_binding_digest:
        raise ObservationRejected("ticket source material digest differs")
    material = json.loads(source_binding_json)
    dimensions = lambda value: None if value is None else asdict(value)
    if (material.get("input_sha256") != input_sha256 or material.get("invocation_key") != invocation_key or
            material.get("selected_route") != dimensions(selection) or material.get("configured_route") != dimensions(selection) or
            material.get("requested_route") != dimensions(requested) or material.get("recommended_route") != dimensions(recommended)):
        raise ObservationRejected("ticket input/key/configuration projection differs from source material")
    ticket = object.__new__(_CallerSourceTicket)
    for name, value in {"observer": observer, "issuer": issuer, "selection": selection,
            "requested": requested, "recommended": recommended, "invocation_key": invocation_key,
            "input_sha256": input_sha256, "source_binding_json": source_binding_json,
            "source_binding_digest": source_binding_digest, "_seal": _CALLER_SOURCE_TICKET_SEAL}.items():
        object.__setattr__(ticket, name, value)
    return ticket


@dataclass(frozen=True, init=False)
class CallerExecutionIntent:
    """Private caller composition output, distinct from fixed Owner-policy cases.

    Only the trusted-source bridge creates these values. The private seal gates
    normal API use; arbitrary code inside the owner process is still trusted.
    No imported JSON or caller boolean authenticates the source result.
    """

    selection: CallerRouteSelection
    prompt: str
    invocation_key: str
    cwd: str
    requested: CallerRouteSelection | None
    recommended: CallerRouteSelection | None
    source_binding_json: str
    source_binding_digest: str
    ephemeral_thread: bool = field(default=False, init=False)
    _seal: Any = field(repr=False, compare=False)
    _source_ticket: Any = field(repr=False, compare=False)

    def __init__(self, *args: Any, **kwargs: Any):
        raise ObservationRejected("caller intent requires the private trusted-source composition")

    @property
    def prompt_digest(self) -> str:
        return _sha(self.prompt)

    @property
    def digest(self) -> str:
        return _sha(_json({"selection": asdict(self.selection), "prompt_sha256": self.prompt_digest,
                          "invocation_key": self.invocation_key, "cwd": self.cwd,
                          "ephemeral_thread": False, "source_binding_digest": self.source_binding_digest,
                          "requested": None if self.requested is None else asdict(self.requested),
                          "recommended": None if self.recommended is None else asdict(self.recommended)}))


def _caller_intent_from_trusted_source(*, source_ticket: _CallerSourceTicket, prompt: str,
        cwd: str) -> CallerExecutionIntent:
    """Private bridge wiring only; not a public source authentication function."""
    if (type(source_ticket) is not _CallerSourceTicket or source_ticket._seal is not _CALLER_SOURCE_TICKET_SEAL or source_ticket.issuer is None or
            getattr(source_ticket.observer, "_caller_source_issuer", None) is not source_ticket.issuer):
        raise ObservationRejected("opaque ticket from the installed caller composition required")
    selection, requested, recommended = source_ticket.selection, source_ticket.requested, source_ticket.recommended
    invocation_key, source_binding_json = source_ticket.invocation_key, source_ticket.source_binding_json
    if selection.provider != "openai":
        raise ObservationRejected("only the owned openai App Server transport is supported")
    _exact(invocation_key, "invocation key")
    _exact(cwd, "cwd")
    if not Path(cwd).is_absolute() or not isinstance(prompt, str) or not prompt.strip() or len(prompt.encode()) > 16384:
        raise ObservationRejected("caller task must have an absolute cwd and bounded prompt")
    if _sha(prompt) != source_ticket.input_sha256 or _sha(source_binding_json) != source_ticket.source_binding_digest:
        raise ObservationRejected("ticket task or source binding differs")
    try:
        material = json.loads(source_binding_json)
        if not isinstance(material, dict) or _json(material) != source_binding_json:
            raise ValueError("noncanonical material")
    except (ValueError, TypeError) as error:
        raise ObservationRejected("canonical trusted-source binding required") from error
    intent = object.__new__(CallerExecutionIntent)
    for name, value in {"selection": selection, "prompt": prompt, "invocation_key": invocation_key,
            "cwd": cwd, "requested": requested, "recommended": recommended,
            "source_binding_json": source_binding_json, "source_binding_digest": _sha(source_binding_json),
            "ephemeral_thread": False, "_seal": _CALLER_INTENT_SEAL, "_source_ticket": source_ticket}.items():
        object.__setattr__(intent, name, value)
    return intent


@dataclass(frozen=True)
class DispatchedConfiguration:
    model: str
    provider: str
    effort: str


@dataclass(frozen=True)
class AppServerReceipt:
    intent_digest: str
    selection: RouteSelection | CallerRouteSelection
    requested: RouteSelection | CallerRouteSelection | None
    recommended: RouteSelection | CallerRouteSelection | None
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
    source_binding_json: str | None = None
    source_binding_digest: str | None = None


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

    def consumed_error(self, filename: str, intent_digest: str) -> ObservationRejected:
        """Diagnostic conflict classification only; disk never authorizes replay."""
        try:
            fd = os.open(filename, os.O_RDONLY | os.O_NOFOLLOW, dir_fd=self.fd)
            try:
                info = os.fstat(fd)
                if not stat.S_ISREG(info.st_mode) or info.st_uid != os.getuid() or info.st_mode & 0o077 or info.st_size > 262144:
                    raise ValueError("invalid reservation permissions or shape")
                raw = os.read(fd, 262145)
            finally:
                os.close(fd)
            saved = json.loads(raw)
            if not isinstance(saved, dict) or not isinstance(saved.get("intent_digest"), str):
                raise ValueError("missing intent binding")
            if saved["intent_digest"] != intent_digest:
                return ObservationRejected("invocation key conflicts with durable reserved intent; no retry")
        except (OSError, ValueError, TypeError):
            return ObservationRejected("invocation key has an unreadable or invalid reservation; no retry")
        return ObservationRejected("invocation key already reserved; no new start or retry")

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
        self._direct_observations: dict[str, tuple[str, AppServerReceipt]] = {}
        self._caller_source_issuer = None

    def _owns_direct_receipt(self, intent: CallerExecutionIntent, receipt: AppServerReceipt) -> bool:
        observed = self._direct_observations.get(_sha(intent.invocation_key))
        return observed is not None and observed[0] == intent.digest and observed[1] is receipt

    @staticmethod
    def _surface_timeout(deadline: float) -> float:
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            raise ObservationRejected("disabled surface inspection deadline exceeded")
        return min(30, remaining)

    def _inventory(self, method: str, params: dict[str, Any], events: list[Any],
                   *, deadline: float | None = None) -> list[Any]:
        rows, seen = [], set()
        for _ in range(16):
            result = self._rpc.request(method, params,
                                       timeout=30 if deadline is None else self._surface_timeout(deadline))
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

    def _resolved_features(self, events: list[Any], flags: tuple[str, ...],
                           thread_id: str | None, deadline: float) -> None:
        # Codex 0.159.0's schema defines this as loaded feature enablement.
        # config/read can retain false overrides while this inventory is true.
        method = "experimentalFeature/list"
        params: dict[str, Any] = {"limit": 100}
        if thread_id is not None:
            params["threadId"] = thread_id
        names: dict[str, bool] = {}
        cursors: set[str] = set()
        stages = {"beta", "underDevelopment", "stable", "deprecated", "removed"}
        for _ in range(16):
            try:
                result = self._rpc.request(method, params, timeout=self._surface_timeout(deadline))
            except Exception as error:
                events.append({"method": method, "error_type": type(error).__name__})
                raise ObservationRejected("resolved feature inventory unavailable") from None
            data = result.get("data") if isinstance(result, dict) else None
            summaries = []
            if isinstance(data, list):
                for row in data[:100]:
                    name = row.get("name") if isinstance(row, dict) else None
                    enabled = row.get("enabled") if isinstance(row, dict) else None
                    stage = row.get("stage") if isinstance(row, dict) else None
                    summaries.append({
                        "row_type": type(row).__name__,
                        "name": name if isinstance(name, str) and name in flags else None,
                        "name_sha256": _sha(name) if isinstance(name, str) else None,
                        "name_type": type(name).__name__,
                        "enabled": enabled if type(enabled) is bool else None,
                        "enabled_type": type(enabled).__name__,
                        "stage": stage if isinstance(stage, str) and stage in stages else None})
            events.append({"method": method, "inventory_shape": {
                "data_type": type(data).__name__, "entries": summaries,
                "entry_count": len(data) if isinstance(data, list) else None}})
            if (not isinstance(result, dict) or not isinstance(data, list) or len(data) > 100
                    or "nextCursor" not in result):
                raise ObservationRejected("malformed resolved feature inventory")
            for row in data:
                if (not isinstance(row, dict) or not isinstance(row.get("name"), str)
                        or not row["name"] or row["name"] != row["name"].strip()
                        or len(row["name"]) > 256 or not row["name"].isascii()
                        or not all(char.isalnum() or char in "_." for char in row["name"])
                        or type(row.get("enabled")) is not bool
                        or not isinstance(row.get("stage"), str) or row["stage"] not in stages):
                    raise ObservationRejected("malformed resolved feature entry")
                if row["name"] in names:
                    raise ObservationRejected("duplicate resolved feature name")
                names[row["name"]] = row["enabled"]
            cursor = result["nextCursor"]
            if cursor is None:
                self._surface_timeout(deadline)
                if any(names.get(flag) is not False for flag in flags):
                    raise ObservationRejected("every required resolved feature must be explicitly disabled")
                return
            if (not isinstance(cursor, str) or not cursor or cursor != cursor.strip()
                    or cursor in cursors):
                raise ObservationRejected("invalid resolved feature pagination")
            cursors.add(cursor)
            params = dict(params, cursor=cursor)
        raise ObservationRejected("resolved feature inventory exceeds bounded pagination")

    def _disabled_surface(self, events: list[Any], cwd: str, thread_id: str | None = None,
                          *, deadline: float) -> None:
        # config/read returns the effective host map, not caller TOML or launch
        # arguments. MCP snake-case shape is confirmed against CLI 0.146.1.
        # Raw overrides are necessary but do not prove resolved enablement.
        response = self._rpc.request("config/read", {"includeLayers": False, "cwd": cwd},
                                     timeout=self._surface_timeout(deadline))
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
        safe_feature_shape = ({flag: {
            "present": flag in features,
            "json_type": ("boolean" if type(features.get(flag)) is bool else
                          "null" if features.get(flag) is None else
                          "object" if isinstance(features.get(flag), dict) else
                          "array" if isinstance(features.get(flag), list) else
                          "string" if isinstance(features.get(flag), str) else "number") if flag in features else "missing",
            "boolean": features.get(flag) if type(features.get(flag)) is bool else None}
            for flag in flags} if isinstance(features, dict) else None)
        web_search = config.get("web_search")
        safe_web_search = web_search if isinstance(web_search, str) and web_search in {
            "disabled", "cached", "live"} else None
        events.append({"method": "config/read", "effective_surface": {
            "mcp_enabled": safe_servers, "plugin_enabled": safe_plugins,
            "features": safe_features, "feature_shape": safe_feature_shape,
            "web_search": safe_web_search}})
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
        self._resolved_features(events, flags, thread_id, deadline)
        params: dict[str, Any] = {"limit": 100, "detail": "full"}
        if thread_id is not None:
            params["threadId"] = thread_id
        rows = self._inventory("mcpServerStatus/list", params, events, deadline=deadline)
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
        self._surface_timeout(deadline)

    @staticmethod
    def _turn(read: Any, intent: ExecutionIntent | CallerExecutionIntent, thread_id: str, turn_id: str) -> dict[str, Any]:
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

    def run_once(self, intent: ExecutionIntent | CallerExecutionIntent, *, completion_timeout: float = 60) -> AppServerReceipt:
        if type(intent) not in (ExecutionIntent, CallerExecutionIntent):
            raise ObservationRejected("immutable execution intent required")
        caller = type(intent) is CallerExecutionIntent
        if caller and (getattr(intent, "_seal", None) is not _CALLER_INTENT_SEAL or
                intent.ephemeral_thread is not False or _sha(intent.source_binding_json) != intent.source_binding_digest):
            raise ObservationRejected("private current-source caller intent required")
        if caller and (type(intent._source_ticket) is not _CallerSourceTicket or
                intent._source_ticket._seal is not _CALLER_SOURCE_TICKET_SEAL or intent._source_ticket.issuer is None or
                intent._source_ticket.observer is not self or intent._source_ticket.issuer is not self._caller_source_issuer):
            raise ObservationRejected("caller intent belongs to another source/observer composition")
        maximum_timeout = 180 if not caller and intent.selection.catalogue_digest in {
            one_off_catalogue().digest, persisted_catalogue().digest} else 60
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

    def _run_reserved(self, intent: ExecutionIntent | CallerExecutionIntent, key_digest: str, timeout: float) -> AppServerReceipt:
        ledger = _Ledger(self._ledger_dir)
        try:
            reservation = {"stage": "reserved",
                         "intent_digest": intent.digest, "prompt_digest": intent.prompt_digest,
                         "invocation_key": intent.invocation_key, "selection": asdict(intent.selection)}
            if type(intent) is CallerExecutionIntent:
                reservation.update(source_binding_json=intent.source_binding_json,
                                   source_binding_digest=intent.source_binding_digest)
            ledger.write(key_digest + ".00-reserved.json", reservation)
        except FileExistsError as error:
            rejection = ledger.consumed_error(key_digest + ".00-reserved.json", intent.digest)
            ledger.close()
            raise rejection from error
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
        seal_attempted = False
        try:
            # One inspection/dispatch budget for the entire owned run. Refreshing
            # the thread's surface never extends the time permitted for a send.
            dispatch_deadline = time.monotonic() + 55
            selection = intent.selection
            models = self._inventory("model/list", {"includeHidden": True, "limit": 100}, events,
                                     deadline=dispatch_deadline)
            matches = [model for model in models if isinstance(model, dict) and model.get("model") == selection.model]
            if len(matches) != 1 or not isinstance(matches[0].get("supportedReasoningEfforts"), list):
                raise ObservationRejected("selected model not uniquely advertised by host")
            efforts = matches[0]["supportedReasoningEfforts"]
            if not any(isinstance(option, dict) and option.get("reasoningEffort") == selection.effort
                       for option in efforts):
                raise ObservationRejected("selected effort not advertised by host")
            self._disabled_surface(events, intent.cwd, deadline=dispatch_deadline)
            params = {"model": selection.model, "modelProvider": "openai", "cwd": intent.cwd,
                      "approvalPolicy": "never", "sandbox": "read-only", "ephemeral": intent.ephemeral_thread,
                      "allowProviderModelFallback": False,
                      "config": {"model_reasoning_effort": selection.effort},
                      "developerInstructions": "Complete only the exact supplied bounded task. Do not use tools, MCP, plugins or agents."}
            events.append({"method": "thread/start", "request": params})
            started = self._rpc.request("thread/start", params, timeout=self._surface_timeout(dispatch_deadline))
            events.append({"method": "thread/start", "response": started})
            if not isinstance(started, dict) or not isinstance(started.get("thread"), dict):
                raise ObservationRejected("missing thread/start host response")
            thread_id = _exact(started["thread"].get("id"), "thread id")
            if started["thread"].get("ephemeral") is not intent.ephemeral_thread:
                raise ObservationRejected("thread/start persistence confirmation mismatch")
            if (started.get("model"), started.get("modelProvider"), started.get("reasoningEffort")) != (
                    selection.model, "openai", selection.effort):
                raise ObservationRejected("thread/start configured model/provider/effort mismatch")
            if (started.get("approvalPolicy") != "never" or started.get("cwd") != intent.cwd or
                    not isinstance(started.get("sandbox"), dict) or started["sandbox"].get("type") != "readOnly"):
                raise ObservationRejected("thread/start execution boundary mismatch")
            configured = DispatchedConfiguration(started["model"], started["modelProvider"], started["reasoningEffort"])
            self._disabled_surface(events, intent.cwd, thread_id, deadline=dispatch_deadline)
            params = {"threadId": thread_id, "model": selection.model, "effort": selection.effort,
                      "clientUserMessageId": intent.invocation_key,
                      "input": [{"type": "text", "text": intent.prompt, "text_elements": []}]}
            self._surface_timeout(dispatch_deadline)
            # This durable fence records send intent, not proof of an RPC send.
            # Expiry during filesystem I/O still prevents the request below.
            ledger.write(key_digest + ".01-before-turn.json", {"stage": "before_turn", "thread_id": thread_id,
                         "intent_digest": intent.digest, "request": params, "events": events})
            self._surface_timeout(dispatch_deadline)
            events.append({"method": "turn/start", "request": params})
            accepted = self._rpc.request("turn/start", params, timeout=self._surface_timeout(dispatch_deadline))
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
            try:
                read = self._rpc.request("thread/read", {"threadId": thread_id, "includeTurns": True}, timeout=30)
            except Exception as error:
                diagnostic = {"error_type": type(error).__name__, "method": "thread/read"}
                if type(error) is AppServerRpcError and type(error.code) is int and -(2**31) <= error.code < 2**31:
                    diagnostic["code"] = error.code
                events.append({"method": "thread/read", "error": diagnostic})
                raise
            events.append({"method": "thread/read", "response": read})
            if outcome != "completed":
                raise ObservationRejected("host task did not complete successfully")
            self._turn(read, intent, thread_id, turn_id)

            try:
                seal_attempted = True
                notifications = self._rpc.seal_notifications(timeout=30)
            except Exception as error:
                raise ObservationRejected(
                    f"complete notification stream capture failed ({type(error).__name__})"
                ) from None
            capture_snapshot_returned = True
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
            notification_capture_complete = self._evidence_kind == "owned_stdio"
            status = "completed_configured_route"
        except ObservationRejected as error:
            status, failure = "configured_route_rejected", str(error)
        except Exception as error:
            failure = f"host transport failure ({type(error).__name__})"
        finally:
            if completion_notification is not None and not seal_attempted:
                seal_attempted = True
                try:
                    notifications = self._rpc.seal_notifications(timeout=30)
                    capture_snapshot_returned = True
                    if not isinstance(notifications, list) or any(not isinstance(value, dict) for value in notifications):
                        raise ObservationRejected("complete notification stream snapshot is malformed")
                    events.extend({"notification": value} for value in notifications)
                    matching = [value for value in notifications
                                if value.get("method") == "turn/completed"
                                and isinstance(value.get("params"), dict)
                                and value["params"].get("threadId") == thread_id
                                and isinstance(value["params"].get("turn"), dict)
                                and value["params"]["turn"].get("id") == turn_id]
                    if len(matching) != 1 or matching[0] != completion_notification:
                        raise ObservationRejected("complete notification stream lacks the unique observed turn completion")
                    notification_capture_complete = self._evidence_kind == "owned_stdio"
                except Exception as error:
                    events.append({"notification_seal_error": type(error).__name__})
                    notification_capture_complete = False
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
                # A later evidence check must not replace the original transport
                # failure; the notification remains retained for inspection.
                if status == "unknown_after_reservation":
                    continue
                if notification.get("method") == "model/rerouted":
                    status, failure = "configured_route_rejected", "host reported model reroute"
                elif isinstance(item, dict) and item.get("type") not in {"userMessage", "agentMessage", "reasoning"}:
                    status, failure = "configured_route_rejected", "host notification reports tool use or unknown item"
            receipt = AppServerReceipt(intent.digest, intent.selection, intent.requested, intent.recommended,
                        intent.prompt_digest, intent.invocation_key, self._evidence_kind, status,
                        thread_id, turn_id, configured, outcome, failure, _json(events),
                        notification_capture_complete,
                        source_binding_json=intent.source_binding_json if type(intent) is CallerExecutionIntent else None,
                        source_binding_digest=intent.source_binding_digest if type(intent) is CallerExecutionIntent else None)
            try:
                ledger.write(key_digest + ".02-observation.json", {"stage": "observation", "receipt": asdict(receipt)})
            finally:
                ledger.close()
        if outcome in {"completed", "failed", "interrupted"}:
            self._terminal[key_digest] = (intent.digest, receipt)
        self._direct_observations[key_digest] = (intent.digest, receipt)
        return receipt
