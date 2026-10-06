"""Private current-source to owned App Server composition for S05.

The public default has no source or host and denies execution. A host composition
root must privately install a source adapter which authenticates the current
caller and validates persisted advisory/selection currentness. The separate
authenticated_caller_source adapter uses the owned authenticated Unix source;
bounded_caller_route_launcher installs it only in a private owner composition.
Neither module imports stored material as authorization or attests native agents.
Canonical source bytes bind the intent; bytes/digests never authenticate a caller.
Private offline source/RPC fixtures prove logic only. Existing consumed fixed
cases and the finite Owner-policy catalogue are separate contracts.
"""

from __future__ import annotations

from abc import ABC, abstractmethod
from dataclasses import dataclass
import hashlib
import json
import re
import unicodedata
from pathlib import Path
from time import time
from uuid import UUID

from scripts.codex_app_server_observer import (AppServerObserver, AppServerReceipt,
    CallerRouteSelection, _caller_intent_from_trusted_source, _mint_caller_source_ticket)


class CallerRoutingRejected(ValueError):
    """No caller task may start through this invalid or unavailable composition."""


def _identifier(value: object, field: str) -> str:
    try:
        if not isinstance(value, str) or str(UUID(value)) != value or UUID(value).int == 0:
            raise ValueError("not a canonical nonzero UUID")
    except (ValueError, AttributeError, TypeError) as error:
        raise CallerRoutingRejected(f"{field} must be a canonical nonzero UUID") from error
    return value


def _key(value: object, field: str, limit: int) -> str:
    if (not isinstance(value, str) or not value or value.strip() != value or
            len(value.encode("utf-8")) > limit or any(unicodedata.category(c) == "Cc" for c in value)):
        raise CallerRoutingRejected(f"{field} must be a bounded exact key")
    return value


def _digest(value: object, field: str) -> str:
    if not isinstance(value, str) or re.fullmatch(r"[0-9a-f]{64}", value) is None:
        raise CallerRoutingRejected(f"{field} must be a lowercase SHA-256")
    return value


def _reference(value: object, field: str, limit: int = 512) -> str:
    if not isinstance(value, str) or not value or value.strip() != value or len(value.encode()) > limit or "\0" in value:
        raise CallerRoutingRejected(f"{field} must be a bounded exact source reference")
    return value


def _route_id(value: object) -> str:
    if not isinstance(value, str) or re.fullmatch(r"[A-Za-z0-9._/:-]{1,128}", value) is None:
        raise CallerRoutingRejected("invalid recorded route or capability ID")
    return value


@dataclass(frozen=True)
class CallerRoutingRequest:
    """Explicit route plus source pins; no actor/session/model/effort authority.

    The source adapter adds input_sha256 from the composition's exact prompt
    bytes when calling PrepareModelRouteHostSelection. Workspace and invoking
    identities are derived from the source adapter's authenticated context.
    """

    preparation_request_key: str
    decision_id: str
    disposition_id: str
    expected_task_id: str
    expected_task_revision: int
    expected_work_context_digest: str
    expected_catalogue_digest: str
    selected_route_id: str
    invocation_key: str

    def __post_init__(self) -> None:
        for name in ("decision_id", "disposition_id", "expected_task_id"):
            _identifier(getattr(self, name), name)
        for name, limit in (("preparation_request_key", 256), ("selected_route_id", 128), ("invocation_key", 256)):
            _key(getattr(self, name), name, limit)
        for name in ("expected_work_context_digest", "expected_catalogue_digest"):
            _digest(getattr(self, name), name)
        if type(self.expected_task_revision) is not int or not 1 <= self.expected_task_revision <= 2**63 - 1:
            raise CallerRoutingRejected("expected_task_revision must be a positive integer")


@dataclass(frozen=True)
class CurrentHostSelection:
    """Direct trusted-source result, never caller input or a stored JSON import.

    Constructing this value supplies no provenance. Only a privately installed
    source port's direct return will be consumed by the composition below.
    """

    material_json: str
    material_sha256: str


class TrustedCurrentSelectionPort(ABC):
    """Private host dependency, not a public authentication or trust marker.

    The installed adapter owns current authenticated caller/session context and
    must re-read current persisted authority, exact decision/accepted disposition,
    Work/Matrix state, catalogue eligibility and task-input binding. Request JSON,
    marker flags and file contents cannot install or replace this adapter.
    Arbitrary code in the owner composition process remains trusted.
    """

    @abstractmethod
    def resolve_current(self, request: CallerRoutingRequest, *, input_sha256: str) -> CurrentHostSelection:
        raise NotImplementedError


def _json(value: object) -> str:
    return json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=False, allow_nan=False)


def _shape(value: object, fields: str) -> dict:
    if not isinstance(value, dict) or set(value) != set(fields.split()):
        raise CallerRoutingRejected("trusted material has unknown or missing fields")
    return value


def _integer(value: object, minimum: int = 0, maximum: int = 2**64 - 1) -> int:
    if type(value) is not int or not minimum <= value <= maximum:
        raise CallerRoutingRejected("trusted material has an invalid integer")
    return value


def _strings(value: object, *, minimum: int = 0, maximum: int = 32, choices: bool = False) -> list[str]:
    if not isinstance(value, list) or any(not isinstance(v, str) or not v or v.strip() != v for v in value) or len(set(value)) != len(value):
        raise CallerRoutingRejected("trusted material has an invalid string set")
    if not minimum <= len(value) <= maximum:
        raise CallerRoutingRejected("trusted material has an invalid string-set size")
    for item in value:
        if choices:
            _reference(item, "Matrix choice", 4096)
        else:
            _route_id(item)
    return value


def _route_record(value: object) -> dict:
    record = _shape(value, "requested_route_id recommended_route_id observed_actual")
    for field in ("requested_route_id", "recommended_route_id"):
        if record[field] is not None:
            _route_id(record[field])
    if record["observed_actual"] is not None:
        raise CallerRoutingRejected("advisory history must not claim an actual serving route")
    return record


def _fact(value: object, kind: str, link: dict, authority: dict) -> object:
    known = _shape(_shape(value, "Known")["Known"], "value provenance")
    actual = known["value"]
    if kind == "number":
        _integer(actual)
    elif kind == "set":
        _strings(actual)
    else:
        _route_id(actual)
    provenance = known["provenance"]
    if not isinstance(provenance, dict) or len(provenance) != 1:
        raise CallerRoutingRejected("unknown work fact provenance")
    variant, fields = next(iter(provenance.items()))
    shapes = {"Caller": "source_ref work_node_id work_node_revision",
              "ConfirmedWorkRequirement": "frozen_snapshot_id requirements_semantic_digest source_ref work_node_id work_node_revision",
              "OperatingEvidence": "source_ref content_digest observed_at_epoch_ms expires_at_epoch_ms work_node_id work_node_revision",
              "Host": "evidence_ref"}
    if variant not in shapes:
        raise CallerRoutingRejected("unknown work fact provenance")
    fields = _shape(fields, shapes[variant])
    if kind == "set":
        if variant != "Host":
            raise CallerRoutingRejected("host capabilities require host provenance")
        _reference(fields["evidence_ref"], "host evidence reference")
    else:
        if variant != "Host":
            _integer(fields["work_node_revision"], 1, 2**63 - 1)
        if variant == "Host" or fields["work_node_id"] != link["mapped_work_node_id"] or fields["work_node_revision"] != link["mapped_work_node_revision"]:
            raise CallerRoutingRejected("work fact is bound to another saved Work revision")
        _reference(fields["source_ref"], "work source reference")
        if variant == "OperatingEvidence":
            _digest(fields["content_digest"], "operating content digest")
            _integer(fields["observed_at_epoch_ms"], 0, 2**63 - 1)
            now = int(time() * 1000)
            if fields["observed_at_epoch_ms"] > now or _integer(fields["expires_at_epoch_ms"], 0, 2**63 - 1) <= max(fields["observed_at_epoch_ms"], now):
                raise CallerRoutingRejected("operating evidence is stale")
        elif variant == "ConfirmedWorkRequirement":
            _identifier(fields["frozen_snapshot_id"], "requirement snapshot")
            _digest(fields["requirements_semantic_digest"], "requirement digest")
            if kind == "number" or any(fields[f] != authority[f] for f in ("frozen_snapshot_id", "requirements_semantic_digest")):
                raise CallerRoutingRejected("work declaration provenance differs from its authority")
    return actual


def _validate_material(snapshot: CurrentHostSelection, request: CallerRoutingRequest, input_sha256: str) -> dict:
    """Closed wire consistency checks after the private source's direct return.

    These checks cannot authenticate arbitrary JSON or establish currentness;
    that authority belongs to the installed source adapter's fresh validation.
    """
    if type(snapshot) is not CurrentHostSelection or not isinstance(snapshot.material_json, str):
        raise CallerRoutingRejected("direct typed current-source result required")
    _digest(snapshot.material_sha256, "producer material digest")
    if hashlib.sha256(snapshot.material_json.encode("utf-8")).hexdigest() != snapshot.material_sha256:
        raise CallerRoutingRejected("producer material digest differs from exact UTF-8 bytes")
    def unique(pairs):
        result = {}
        for key, value in pairs:
            if key in result:
                raise CallerRoutingRejected("duplicate source material field")
            result[key] = value
        return result
    def no_number(value):
        raise CallerRoutingRejected("source material permits no float or nonfinite numbers")
    try:
        material = json.loads(snapshot.material_json, object_pairs_hook=unique,
                              parse_float=no_number, parse_constant=no_number)
        if _json(material) != snapshot.material_json:
            raise CallerRoutingRejected("producer material is not canonical compact UTF-8 JSON")
        return _check_material(material, request, input_sha256)
    except (KeyError, TypeError, ValueError, OverflowError) as error:
        if isinstance(error, CallerRoutingRejected):
            raise
        raise CallerRoutingRejected("malformed trusted current-source material") from None


def _check_material(material: object, request: CallerRoutingRequest, input_sha256: str) -> dict:
    m = _shape(material, "schema intended_host_kind workspace_id invoking_actor_id invoking_session_id preparation decision disposition source_binding requested_route recommended_route selected_route configured_route input_sha256 invocation_key")
    if m["schema"] != "tect.model-route-host-selection/1" or m["intended_host_kind"] != "codex_app_server_owned_stdio":
        raise CallerRoutingRejected("source contract or intended host differs")
    for field in ("workspace_id", "invoking_actor_id", "invoking_session_id"):
        _identifier(m[field], field)
    if m["input_sha256"] != input_sha256 or m["invocation_key"] != request.invocation_key:
        raise CallerRoutingRejected("source input or invocation binding differs")
    p = _shape(m["preparation"], "workspace_id request_key origin_session_id session_preference request_preference advisory_config_revision work catalogue eligible preparation routes")
    if p["workspace_id"] != m["workspace_id"] or p["request_key"] != request.preparation_request_key or p["preparation"] != "Prepared" or p["session_preference"] != "use_workspace" or p["request_preference"] != "use_workspace":
        raise CallerRoutingRejected("source preparation is unavailable or differs")
    _identifier(p["origin_session_id"], "origin session")
    _integer(p["advisory_config_revision"], 0, 2**63 - 1)
    pr = _route_record(p["routes"])
    if pr["recommended_route_id"] is not None:
        raise CallerRoutingRejected("preparation cannot contain a recommendation")
    work = _shape(p["work"], "approved_matrix_selection selection_link context_authority role tool data_class host_capabilities remaining_budget_units available_latency_ms")
    matrix = _shape(work["approved_matrix_selection"], "disposition_id task_id task_revision selected_choice_id expected_input_digest expected_choice_set_digest expected_verification_digest mapped_draft_node_indices")
    for field in ("disposition_id", "task_id"):
        _identifier(matrix[field], field)
    for field in ("expected_input_digest", "expected_choice_set_digest", "expected_verification_digest"):
        _digest(matrix[field], field)
    _reference(matrix["selected_choice_id"], "Matrix choice ID", 4096)
    if matrix["task_id"] != request.expected_task_id or matrix["task_revision"] != request.expected_task_revision:
        raise CallerRoutingRejected("source task revision differs from caller pin")
    _integer(matrix["task_revision"], 1, 2**63 - 1)
    indices = matrix["mapped_draft_node_indices"]
    if not isinstance(indices, list) or not 1 <= len(indices) <= 100:
        raise CallerRoutingRejected("invalid Matrix mapped indices")
    for index in indices:
        _integer(index)
    if any(left >= right for left, right in zip(indices, indices[1:])):
        raise CallerRoutingRejected("Matrix mapped indices must be strictly increasing")
    link = _shape(work["selection_link"], "candidate_set_id caller_request_id mapped_draft_node_index mapped_work_node_id mapped_work_node_revision")
    for field in ("candidate_set_id", "caller_request_id", "mapped_work_node_id"):
        _identifier(link[field], field)
    _integer(link["mapped_work_node_revision"], 1, 2**63 - 1)
    if _integer(link["mapped_draft_node_index"]) not in indices:
        raise CallerRoutingRejected("mapped Work is absent from the original Matrix selection")
    authority = _shape(work["context_authority"], "frozen_snapshot_id authority_schema requirements_semantic_digest operating_verification_digest")
    _identifier(authority["frozen_snapshot_id"], "frozen snapshot")
    _digest(authority["requirements_semantic_digest"], "requirements digest")
    if authority["authority_schema"] != "tect.matrix-requirements/1" or authority["operating_verification_digest"] != matrix["expected_verification_digest"]:
        raise CallerRoutingRejected("Matrix context authority differs")
    facts = {name: _fact(work[name], kind, link, authority) for name, kind in (
        ("role", "text"), ("tool", "text"), ("data_class", "text"), ("host_capabilities", "set"),
        ("remaining_budget_units", "number"), ("available_latency_ms", "number"))}
    c = _shape(p["catalogue"], "schema version routes")
    if c["schema"] != "tect.model-routes/1" or not isinstance(c["routes"], list) or len(c["routes"]) > 64:
        raise CallerRoutingRejected("invalid recorded route catalogue")
    _integer(c["version"], 1)
    catalogue = {}
    eligible_ids = []
    for row in c["routes"]:
        row = _shape(row, "id provider model effort enabled allowed_matrix_choice_ids allowed_roles allowed_tools allowed_data_classes required_host_capabilities minimum_budget_units minimum_latency_ms")
        for field in ("id", "provider", "model", "effort"):
            _route_id(row[field])
        if row["id"] in catalogue or type(row["enabled"]) is not bool:
            raise CallerRoutingRejected("duplicate or invalid recorded route")
        _strings(row["allowed_matrix_choice_ids"], minimum=1, choices=True)
        for field in ("allowed_roles", "allowed_tools", "allowed_data_classes", "required_host_capabilities"):
            _strings(row[field], minimum=0 if field == "required_host_capabilities" else 1)
        _integer(row["minimum_budget_units"])
        _integer(row["minimum_latency_ms"])
        catalogue[row["id"]] = {"route_id": row["id"], **{f: row[f] for f in ("provider", "model", "effort")}}
        if (row["enabled"] and matrix["selected_choice_id"] in row["allowed_matrix_choice_ids"] and
                all(facts[f] in row[allowed] for f, allowed in (("role", "allowed_roles"), ("tool", "allowed_tools"), ("data_class", "allowed_data_classes"))) and
                set(row["required_host_capabilities"]).issubset(facts["host_capabilities"]) and
                facts["remaining_budget_units"] >= row["minimum_budget_units"] and facts["available_latency_ms"] >= row["minimum_latency_ms"]):
            eligible_ids.append(row["id"])
    e = _shape(p["eligible"], "catalogue_version catalogue_digest work_context_digest configured_route_ids route_ids")
    _integer(e["catalogue_version"], 1)
    if (e["catalogue_version"] != c["version"] or e["catalogue_digest"] != request.expected_catalogue_digest or
            e["work_context_digest"] != request.expected_work_context_digest or e["configured_route_ids"] != sorted(catalogue) or e["route_ids"] != sorted(eligible_ids)):
        raise CallerRoutingRejected("eligible catalogue/work binding differs")
    d = _shape(m["decision"], "id prepared input outcome routes")
    if d["id"] != request.decision_id or _json(d["prepared"]) != _json(p):
        raise CallerRoutingRejected("decision does not retain the exact preparation")
    dr = _route_record(d["routes"])
    recommended = _shape(_shape(d["outcome"], "Recommended")["Recommended"], "route_id")["route_id"]
    ranking = _shape(_shape(d["input"], "Ranking")["Ranking"], "catalogue_digest work_context_digest ranked_route_ids")
    ranks = _strings(ranking["ranked_route_ids"], minimum=1, maximum=64)
    if (not ranks or sorted(ranks) != sorted(eligible_ids) or ranks[0] != recommended or
            recommended != dr["recommended_route_id"] or dr["requested_route_id"] != pr["requested_route_id"] or
            ranking["catalogue_digest"] != e["catalogue_digest"] or ranking["work_context_digest"] != e["work_context_digest"]):
        raise CallerRoutingRejected("decision ranking/recommendation binding differs")
    disposition = _shape(m["disposition"], "id decision_id workspace_id actor_id action rationale")
    if (disposition["id"] != request.disposition_id or disposition["decision_id"] != request.decision_id or
            disposition["workspace_id"] != m["workspace_id"] or disposition["actor_id"] != m["invoking_actor_id"] or disposition["action"] != "Accept"):
        raise CallerRoutingRejected("current caller lacks the exact accepted disposition")
    rationale = disposition["rationale"]
    if not isinstance(rationale, str) or not rationale.strip() or len(rationale.encode()) > 4096 or "\0" in rationale:
        raise CallerRoutingRejected("invalid persisted disposition rationale")
    binding = _shape(m["source_binding"], "locator source_request_id source_recorded_by_actor_id source_recorded_by_session_id frozen_snapshot_id authority_schema requirements_semantic_digest matrix_save_actor_id matrix_save_session_id scope_id result_revision matrix_evaluation_digest matrix_catalogue_version")
    for field in ("source_request_id", "source_recorded_by_actor_id", "source_recorded_by_session_id", "frozen_snapshot_id", "matrix_save_actor_id", "matrix_save_session_id", "scope_id"):
        _identifier(binding[field], field)
    _integer(binding["result_revision"], 1, 2**63 - 1)
    _digest(binding["matrix_evaluation_digest"], "Matrix evaluation digest")
    if not isinstance(binding["matrix_catalogue_version"], str) or not binding["matrix_catalogue_version"]:
        raise CallerRoutingRejected("missing Matrix catalogue version")
    for field in ("frozen_snapshot_id", "authority_schema", "requirements_semantic_digest"):
        if binding[field] != authority[field]:
            raise CallerRoutingRejected("original source binding differs from frozen context")
    locator = binding["locator"]
    level = locator.get("level") if isinstance(locator, dict) else None
    variants = {"program": "level program_id", "scope": "level program_id scope_id", "slice": "level program_id scope_id candidate_set_id work_candidate_id expected_work_revision", "opened_slice": "level slice_id"}
    if level not in variants:
        raise CallerRoutingRejected("unknown original source locator")
    locator = _shape(locator, variants[level])
    for field in set(locator) - {"level", "expected_work_revision"}:
        _identifier(locator[field], field)
    if level == "slice":
        _integer(locator["expected_work_revision"], 1, 2**63 - 1)
    for field, route_id in (("requested_route", pr["requested_route_id"]), ("recommended_route", recommended), ("selected_route", request.selected_route_id), ("configured_route", request.selected_route_id)):
        expected = catalogue.get(route_id) if route_id is not None else None
        if expected is None and route_id is not None or _json(m[field]) != _json(expected):
            raise CallerRoutingRejected("route dimension differs from the exact recorded catalogue")
    if request.selected_route_id not in eligible_ids or m["selected_route"]["provider"] != "openai":
        raise CallerRoutingRejected("selected route is ineligible for the owned host")
    return m


class CallerHostRouting:
    """Default-deny public surface; private composition is installed by its host."""

    def __init__(self):
        self._source = None
        self._observer = None
        self._source_ticket_issuer = object()

    def _install(self, source: TrustedCurrentSelectionPort, observer: AppServerObserver) -> None:
        if not isinstance(source, TrustedCurrentSelectionPort) or type(observer) is not AppServerObserver:
            raise CallerRoutingRejected("private trusted-source and exact observer wiring required")
        self._source = source
        self._observer = observer
        observer._caller_source_issuer = self._source_ticket_issuer

    def run_once(self, request: CallerRoutingRequest, *, prompt: str, cwd: str) -> AppServerReceipt:
        if self._source is None or self._observer is None:
            raise CallerRoutingRejected("no trusted current-source/host composition installed")
        if type(request) is not CallerRoutingRequest or not isinstance(prompt, str) or not prompt.strip() or len(prompt.encode("utf-8")) > 16384 or not isinstance(cwd, str) or not Path(cwd).is_absolute() or cwd.strip() != cwd:
            raise CallerRoutingRejected("immutable caller pins and bounded exact task required")
        input_sha256 = hashlib.sha256(prompt.encode("utf-8")).hexdigest()
        try:
            snapshot = self._source.resolve_current(request, input_sha256=input_sha256)
        except Exception:
            raise CallerRoutingRejected("trusted current-source selection is unavailable") from None
        material = _validate_material(snapshot, request, input_sha256)
        dimension = lambda value: None if value is None else CallerRouteSelection(**value)
        ticket = _mint_caller_source_ticket(observer=self._observer, issuer=self._source_ticket_issuer,
            selection=dimension(material["selected_route"]), requested=dimension(material["requested_route"]),
            recommended=dimension(material["recommended_route"]), invocation_key=material["invocation_key"],
            input_sha256=input_sha256, source_binding_json=snapshot.material_json, source_binding_digest=snapshot.material_sha256)
        intent = _caller_intent_from_trusted_source(source_ticket=ticket, prompt=prompt, cwd=cwd)
        receipt = self._observer.run_once(intent)
        if (type(receipt) is not AppServerReceipt or not self._observer._owns_direct_receipt(intent, receipt) or
                receipt.intent_digest != intent.digest or receipt.prompt_digest != input_sha256 or
                receipt.invocation_key != intent.invocation_key or receipt.source_binding_json != snapshot.material_json or
                receipt.source_binding_digest != snapshot.material_sha256 or receipt.selection != intent.selection or
                receipt.requested != intent.requested or receipt.recommended != intent.recommended):
            raise CallerRoutingRejected("receipt is not the exact direct owned-observer result")
        return receipt
