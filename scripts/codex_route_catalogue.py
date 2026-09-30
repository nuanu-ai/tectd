"""Finite Owner-policy development routes for the new Codex App Server case.

This catalogue is configuration, not authentication, dispatch authorization or
proof of model availability. It makes no price, latency or capability claims.
No route is chosen automatically and this module has no transport effects.
Historical agents.spawn_agent receipts and route-a decisions are separate.
"""

from __future__ import annotations

import hashlib
import json
import re
from dataclasses import asdict, dataclass
from typing import Any


class SelectionRejected(ValueError):
    """Explicit selection does not match the finite development policy."""


def _canonical(value: Any) -> str:
    return json.dumps(value, sort_keys=True, separators=(",", ":"),
                      ensure_ascii=False, allow_nan=False)


def _digest(value: str) -> str:
    return hashlib.sha256(value.encode("utf-8")).hexdigest()


@dataclass(frozen=True)
class Route:
    route_id: str
    model: str
    effort: str
    purpose: str


@dataclass(frozen=True)
class Catalogue:
    schema: str
    version: int
    policy_source: str
    host_transport: str
    model_availability: str
    routes: tuple[Route, ...]

    @property
    def canonical_json(self) -> str:
        return _canonical(asdict(self))

    @property
    def digest(self) -> str:
        return _digest(self.canonical_json)


_CATALOGUE = Catalogue(
    schema="codex-app-server-development-route-catalogue-v1",
    version=1,
    policy_source="Tony current-task Owner policy: Sol orchestration and bounded executors",
    host_transport="codex-app-server-stdio",
    model_availability="unverified",
    routes=(
        Route("codex-app-server-implementation-sol61-medium-v1",
              "gpt-6.1-sol", "medium", "ordinary_implementation"),
        Route("codex-app-server-routine-luna-xhigh-v1",
              "gpt-6-luna", "xhigh", "routine"),
    ),
)


def development_catalogue() -> Catalogue:
    """Return the immutable finite policy; runtime availability needs observation."""
    return _CATALOGUE


ONE_OFF_CASE_ID = "s05-appserver-luna56-oneoff-7f29a6f6"
ONE_OFF_INVOCATION_KEY = "s05-owner-luna56-oneoff-7f29a6f6"
_ONE_OFF_CATALOGUE = Catalogue(
    schema="codex-app-server-case-scoped-one-off-route-catalogue-v1", version=1,
    policy_source="Tony current-task approval: one isolated S05 Luna56 case 7f29a6f6",
    host_transport="codex-app-server-stdio", model_availability="unverified",
    routes=(Route("codex-app-server-one-off-luna56-xhigh-7f29a6f6",
                  "gpt-5.6-luna", "xhigh", "routine"),),
)


def one_off_catalogue() -> Catalogue:
    """Fixed root composition data, not authentication or serving telemetry."""
    return _ONE_OFF_CATALOGUE


def one_off_marker_json() -> str:
    return _canonical({"kind": "S05_APP_SERVER_MARKER_V1", "case_id": ONE_OFF_CASE_ID,
                       "selected_route_id": _ONE_OFF_CATALOGUE.routes[0].route_id,
                       "catalogue_sha256": _ONE_OFF_CATALOGUE.digest})


def one_off_prompt() -> str:
    return ("Perform this bounded no-tools JSON echo task. Return exactly the single JSON object "
            "below, with no Markdown, commentary or additional fields. Do not use tools, web, "
            "files, MCP, plugins or agents. The object is a supplied case marker, not a claim "
            "about your model identity.\n" + one_off_marker_json())


@dataclass(frozen=True)
class RouteSelection:
    catalogue_schema: str
    catalogue_version: int
    catalogue_digest: str
    policy_source: str
    host_transport: str
    route_id: str
    model: str
    effort: str
    purpose: str
    task_input_digest: str

    def __post_init__(self) -> None:
        identity = (self.catalogue_schema, self.catalogue_version, self.catalogue_digest,
                    self.policy_source, self.host_transport)
        matches_catalogue = [candidate for candidate in (_CATALOGUE, _ONE_OFF_CATALOGUE)
                             if identity == (candidate.schema, candidate.version, candidate.digest,
                                             candidate.policy_source, candidate.host_transport)]
        if len(matches_catalogue) != 1:
            raise SelectionRejected("selection catalogue or host transport differs from policy")
        catalogue = matches_catalogue[0]
        if type(self.catalogue_version) is not int:
            raise SelectionRejected("catalogue version must be an integer")
        matches = [route for route in catalogue.routes if route.route_id == self.route_id]
        if len(matches) != 1:
            raise SelectionRejected("route is not listed in development policy")
        route = matches[0]
        if (self.model, self.effort, self.purpose) != (route.model, route.effort, route.purpose):
            raise SelectionRejected("model, effort or purpose differs from selected route")
        if not isinstance(self.task_input_digest, str) or re.fullmatch(
                r"[0-9a-f]{64}", self.task_input_digest) is None:
            raise SelectionRejected("task input digest must be lowercase SHA-256")
        if catalogue is _ONE_OFF_CATALOGUE and self.task_input_digest != _digest(one_off_prompt()):
            raise SelectionRejected("one-off selection requires the exact fixed case prompt")

    @property
    def canonical_json(self) -> str:
        return _canonical(asdict(self))

    @property
    def digest(self) -> str:
        """Bind explicit route configuration, purpose and exact task-input digest."""
        return _digest(self.canonical_json)


def select_route(*, route_id: str, model: str, effort: str, purpose: str,
                 task_input_digest: str, catalogue_version: int,
                 catalogue_digest: str) -> RouteSelection:
    """Validate one explicit selection; caller digest is binding, not provenance.

    The host must independently prepare/check the exact task input, establish
    authority and observe execution. This function supplies none of those facts.
    """
    catalogue = development_catalogue()
    return RouteSelection(
        catalogue_schema=catalogue.schema, catalogue_version=catalogue_version,
        catalogue_digest=catalogue_digest, policy_source=catalogue.policy_source,
        host_transport=catalogue.host_transport, route_id=route_id, model=model,
        effort=effort, purpose=purpose, task_input_digest=task_input_digest,
    )


def select_one_off_route(*, task_input_digest: str) -> RouteSelection:
    """Select only the approved fixed case; human authority stays at root invocation."""
    catalogue = one_off_catalogue()
    route = catalogue.routes[0]
    return RouteSelection(catalogue.schema, catalogue.version, catalogue.digest,
                          catalogue.policy_source, catalogue.host_transport, route.route_id,
                          route.model, route.effort, route.purpose, task_input_digest)
