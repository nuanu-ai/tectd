"""Additive native agents.spawn_agent receipt contract; no dispatch transport.

Host code must privately install the trusted evidence port from its composition
root, outside caller input. This package ships no production composition root or
API; the default verifier denies every receipt. Constructor injection is trusted
dependency wiring, not authentication: arbitrary Python code can implement a
port and fabricate an observation. Offline tests inject a frozen fake, which
exercises verifier logic but proves no host provenance. A real port must read
host command, acceptance and terminal evidence independently. Configured command
values never prove the actual serving model.
Historical S05 route-a and its old decision are not selections of this contract.
"""

from __future__ import annotations

import hashlib
import json
from abc import ABC, abstractmethod
from dataclasses import dataclass
from typing import Any


class ReceiptRejected(ValueError):
    """Receipt does not establish the claimed native host dispatch."""


def _exact(value: Any, field: str) -> str:
    if not isinstance(value, str) or not value or value != value.strip():
        raise ReceiptRejected(f"{field} must be a nonempty exact string")
    return value


def _digest(value: str) -> str:
    return hashlib.sha256(value.encode("utf-8")).hexdigest()


def _json(value: Any) -> str:
    return json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=False,
                      allow_nan=False)


@dataclass(frozen=True)
class Configuration:
    model: str
    effort: str

    def __post_init__(self) -> None:
        _exact(self.model, "model")
        _exact(self.effort, "effort")


@dataclass(frozen=True)
class DispatchIntent:
    """Immutable task input and distinct routing stages, before host dispatch.

    task_input_json is the canonical native message/task_name packet. Its hash
    and all routing stages contribute to the immutable intent digest.
    """

    intent_id: str
    task_input_json: str
    selected_route_id: str
    selected_configuration: Configuration
    requested_route_id: str | None = None
    recommended_route_id: str | None = None
    requested_configuration: Configuration | None = None
    recommended_configuration: Configuration | None = None

    def __post_init__(self) -> None:
        for field in ("intent_id", "selected_route_id"):
            _exact(getattr(self, field), field)
        if type(self.selected_configuration) is not Configuration:
            raise ReceiptRejected("selected_configuration must be immutable Configuration")
        for route_field, config_field in (
            ("requested_route_id", "requested_configuration"),
            ("recommended_route_id", "recommended_configuration"),
        ):
            route = getattr(self, route_field)
            config = getattr(self, config_field)
            if (route is None) != (config is None):
                raise ReceiptRejected(f"{route_field} and {config_field} must both be present or null")
            if route is not None:
                _exact(route, route_field)
            if config is not None and type(config) is not Configuration:
                raise ReceiptRejected(f"{config_field} must be immutable Configuration or null")
        try:
            packet = json.loads(self.task_input_json)
            if not isinstance(packet, dict) or set(packet) != {"message", "task_name"}:
                raise ReceiptRejected("task packet must contain message and task_name")
            for field in ("message", "task_name"):
                _exact(packet[field], field)
            if _json(packet) != self.task_input_json:
                raise ReceiptRejected("task input must be canonical JSON")
        except (TypeError, ValueError) as error:
            raise ReceiptRejected("invalid canonical task input") from error

    @classmethod
    def from_packet(cls, *, task_input: dict[str, str], **fields: Any) -> DispatchIntent:
        return cls(task_input_json=_json(task_input), **fields)

    @property
    def task_input_digest(self) -> str:
        return _digest(self.task_input_json)

    @property
    def digest(self) -> str:
        fields = {field: getattr(self, field) for field in
                  ("intent_id", "requested_route_id", "recommended_route_id", "selected_route_id")}
        fields["task_input_digest"] = self.task_input_digest
        for field in ("requested_configuration", "recommended_configuration", "selected_configuration"):
            config = getattr(self, field)
            fields[field] = None if config is None else {"model": config.model, "effort": config.effort}
        return _digest(_json(fields))


@dataclass(frozen=True)
class NativeCommandMetadata:
    """Values recorded from host dispatch command arguments, not model output."""

    tool: str
    model: str
    reasoning_effort: str
    fork_turns: str
    task_input_digest: str


@dataclass(frozen=True)
class NativeDispatchReceipt:
    host_task_id: str
    intent_digest: str
    task_input_digest: str
    selected_route_id: str
    command: NativeCommandMetadata
    host_accepted: bool
    terminal_outcome: str
    observed_actual: Configuration | None = None


class TrustedHostEvidencePort(ABC):
    """Trusted composition-root dependency; this interface does not authenticate.

    Implementations must retrieve independent host records for this exact intent
    and returned agents.spawn_agent task ID. The host must privately install a
    trusted implementation outside caller input; implementing this public ABC is
    not itself proof of trust. None means no trusted observation, including
    unknown host acceptance.
    """

    @abstractmethod
    def observe(self, *, intent: DispatchIntent, host_task_id: str) -> NativeDispatchReceipt | None:
        raise NotImplementedError


@dataclass(frozen=True)
class VerifiedNativeDispatch:
    intent: DispatchIntent
    receipt: NativeDispatchReceipt

    @property
    def dispatched_configured(self) -> Configuration:
        return Configuration(self.receipt.command.model, self.receipt.command.reasoning_effort)

    @property
    def observed_actual(self) -> Configuration | None:
        return self.receipt.observed_actual


class NativeReceiptVerifier:
    """Fail closed without privately installed host evidence; in-memory duplicate scope.

    This is verification only, not authority to dispatch or proof of at-most-once
    execution. No production composition root or API installs a port here. The
    host must privately wire a trusted port; constructor injection itself is not
    an authentication boundary. Retain a single verifier for its duplicate-
    checking scope; a future host integration must provide durable conflict
    detection across restarts.
    """

    def __init__(self, *, host_evidence: TrustedHostEvidencePort | None = None):
        if host_evidence is not None and not isinstance(host_evidence, TrustedHostEvidencePort):
            raise ReceiptRejected("host evidence must implement the trusted port interface")
        self._host_evidence = host_evidence
        self._verified: dict[str, NativeDispatchReceipt] = {}

    def verify(self, intent: DispatchIntent, receipt: NativeDispatchReceipt) -> VerifiedNativeDispatch:
        if self._host_evidence is None:
            raise ReceiptRejected("no trusted host evidence port installed")
        if type(intent) is not DispatchIntent or type(receipt) is not NativeDispatchReceipt:
            raise ReceiptRejected("immutable intent and native receipt required")
        task_id = _exact(receipt.host_task_id, "host_task_id")
        previous = self._verified.get(task_id)
        if previous is not None and previous != receipt:
            raise ReceiptRejected("conflicting duplicate receipt for host task")
        if receipt.intent_digest != intent.digest or receipt.task_input_digest != intent.task_input_digest:
            raise ReceiptRejected("receipt intent or task input digest mismatch")
        if receipt.selected_route_id != intent.selected_route_id:
            raise ReceiptRejected("receipt selected route mismatch")
        command = receipt.command
        if type(command) is not NativeCommandMetadata or command.tool != "agents.spawn_agent":
            raise ReceiptRejected("native spawn command metadata required")
        if command.task_input_digest != intent.task_input_digest or command.fork_turns != "none":
            raise ReceiptRejected("native command task input or fork binding mismatch")
        if (command.model, command.reasoning_effort) != (
                intent.selected_configuration.model, intent.selected_configuration.effort):
            raise ReceiptRejected("dispatched configured model or effort differs from selection")
        if receipt.host_accepted is not True:
            raise ReceiptRejected("successful host acceptance required")
        if receipt.terminal_outcome not in {"completed", "failed", "cancelled"}:
            raise ReceiptRejected("terminal host outcome required")
        if receipt.observed_actual is not None and type(receipt.observed_actual) is not Configuration:
            raise ReceiptRejected("actual telemetry must be configuration or unknown")
        try:
            observed = self._host_evidence.observe(intent=intent, host_task_id=task_id)
        except Exception as error:
            raise ReceiptRejected("trusted host evidence unavailable") from error
        if type(observed) is not NativeDispatchReceipt or observed != receipt:
            raise ReceiptRejected("receipt differs from independent trusted host observation")
        self._verified[task_id] = receipt
        return VerifiedNativeDispatch(intent, receipt)
