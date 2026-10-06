"""Explicit planning reads; compact mutation receipts are never expanded or overlaid.

Only byte continuations are followed here. EOF never executes collection or
terminal actions. Raw MCP envelope sizes remain the fixture wire capture's job.
"""
from __future__ import annotations
from copy import deepcopy
from dataclasses import dataclass
import hashlib
import json
import re
import uuid

MAX_BYTES = 8 * 1024 * 1024
MAX_PAGES = 8192
ROUTES = {"scope.candidates.context", "scope.context", "slice.candidates.context"}


def require(condition, message):
    if not condition:
        raise AssertionError(message)


def positive(value):
    return type(value) is int and value > 0


def identifier(value):
    require(isinstance(value, str), "missing UUID pin")
    try:
        require(str(uuid.UUID(value)) == value, "noncanonical UUID pin")
    except ValueError as error:
        raise AssertionError("invalid UUID pin") from error


def request(route, params):
    require(route in ROUTES and isinstance(params, dict), "unsupported planning read")
    identifier(params.get("candidate_set_id" if route == "scope.candidates.context" else "scope_id"))
    if route != "scope.context":
        require(isinstance(params.get("view"), str), "missing planning view")
    return {"route": route, "params": deepcopy(params)}


def ready_arguments(action):
    require(isinstance(action, dict) and set(action) == {"kind", "tool", "arguments"}
            and action["kind"] == "ready_call" and action["tool"] == "query", "read destination must be Ready query")
    arguments = action["arguments"]
    require(isinstance(arguments, dict) and set(arguments) == {"route", "params"}, "malformed Ready query")
    return request(arguments["route"], arguments["params"])


def semantic_actions(page):
    actions = page.get("actions")
    require(isinstance(actions, list) and "recommended_action" in page, "missing actual actions or recommendation")
    recommended = page["recommended_action"]
    require(recommended is None or (type(recommended) is int and 0 <= recommended < len(actions)),
            "invalid actual action recommendation")
    return {"terminal_actions": deepcopy(actions), "terminal_recommended_action": recommended}


def resolved_source(route, value, source):
    if source is None:
        return
    if route == "scope.candidates.context":
        context = value["context"]
        require(context["candidate_set"]["id"] == source["candidate_set_id"]
                and context["candidate_set"]["revision"] == source["candidate_set_revision"]
                and context["snapshot"]["id"] == source["snapshot_id"], "assembled candidate source mismatch")
    elif route == "scope.context":
        require(value["id"] == source["scope_id"] and value["revision"] == source["scope_revision"],
                "assembled Scope source mismatch")
    else:
        require(value["scope"]["id"] == source["scope_id"]
                and value["candidate_set"]["id"] == source["candidate_set_id"]
                and value["snapshot"]["id"] == source["snapshot_id"], "assembled planning source mismatch")


@dataclass(frozen=True)
class ResolvedRead:
    value: dict
    provenance: dict


def read_query(call, route, params, *, advertised_action=None):
    """Read an explicit query or the exact separately retained advertised action."""
    initial = request(route, params)
    if advertised_action is not None:
        require(ready_arguments(advertised_action) == initial, "destination differs from advertised query")
    require(not any(k in params for k in ("offset_bytes", "representation_digest")), "initial read must start at byte zero")
    current = deepcopy(initial)
    original_selectors = deepcopy(params)
    continuation_selectors = None
    data = bytearray()
    source = digest = total = None
    maximum_payload = 0
    for pages in range(1, MAX_PAGES + 1):
        page, failed = call("query", current)
        require(not failed and isinstance(page, dict), "planning query failed")
        actual_actions = semantic_actions(page)
        # This bounds individual parsed wire payloads; only capture knows envelope bytes.
        size = len(json.dumps(page, ensure_ascii=False, separators=(",", ":")).encode("utf-8"))
        require(size <= 8192, "planning payload exceeds wire envelope budget")
        maximum_payload = max(maximum_payload, size)
        if page.get("kind") != "fragment":
            require(not data and digest is None, "fragment changed to ordinary JSON")
            return ResolvedRead(page, {"initial_query": initial, "advertised_action": deepcopy(advertised_action),
                                      "source": None, "representation_digest": None, "pages": pages,
                                      "maximum_payload_bytes": maximum_payload, **actual_actions})
        require(page.get("format") == "json" and page.get("encoding") == "utf-8", "invalid JSON fragment encoding")
        ps = page.get("source")
        require(isinstance(ps, dict), "missing fragment source")
        identity = "candidate_set_id" if route == "scope.candidates.context" else "scope_id"
        require(ps.get(identity) == params[identity], "fragment identity changed")
        uuid_pins = {"scope.candidates.context": ("candidate_set_id", "snapshot_id"),
                     "scope.context": ("scope_id",),
                     "slice.candidates.context": ("scope_id", "candidate_set_id", "snapshot_id")}[route]
        for key in uuid_pins:
            identifier(ps.get(key))
        revision_pin = {"scope.candidates.context": "candidate_set_revision", "scope.context": "scope_revision"}.get(route)
        if revision_pin:
            require(positive(ps.get(revision_pin)), "invalid fragment source revision")
            if revision_pin in params:
                require(ps[revision_pin] == params[revision_pin], "initial revision pin mismatch")
        pd, pt = page.get("representation_digest"), page.get("total_bytes")
        require(isinstance(pd, str) and re.fullmatch(r"[a-f0-9]{64}", pd), "invalid representation digest")
        require(type(pt) is int and 0 <= pt <= MAX_BYTES, "representation byte budget exceeded")
        if source is None:
            source, digest, total = deepcopy(ps), pd, pt
        else:
            require((ps, pd, pt) == (source, digest, total), "fragment source, digest or total changed")
        offset, count, text = page.get("offset_bytes"), page.get("returned_bytes"), page.get("text")
        require(type(offset) is int and offset == len(data), "noncontiguous fragment")
        require(type(count) is int and 0 <= count <= 4096 and isinstance(text, str), "invalid fragment byte count")
        encoded = text.encode("utf-8")
        require(count == len(encoded) and offset + count <= total, "fragment byte bounds mismatch")
        data.extend(encoded)
        next_offset = page.get("next_offset_bytes")
        if next_offset is None:
            require("next_offset_bytes" in page and len(data) == total, "premature fragment EOF")
            require(hashlib.sha256(data).hexdigest() == digest, "assembled representation digest mismatch")
            value = json.loads(data)
            require(isinstance(value, dict), "planning representation must be object")
            resolved_source(route, value, source)
            return ResolvedRead(value, {"initial_query": initial, "advertised_action": deepcopy(advertised_action),
                                       "source": source, "representation_digest": digest, "pages": pages,
                                       "maximum_payload_bytes": maximum_payload, **actual_actions})
        require(type(next_offset) is int and count > 0 and next_offset == len(data), "fragment failed to advance")
        actions = page.get("actions")
        require(isinstance(actions, list), "missing byte continuation actions")
        matches = []
        for action in actions:
            if not isinstance(action, dict) or action.get("kind") != "ready_call" or action.get("tool") != "query":
                continue
            args = action.get("arguments", {})
            ap = args.get("params", {})
            if (args.get("route") == route and isinstance(ap, dict)
                    and ap.get("offset_bytes") == next_offset and ap.get("representation_digest") == digest
                    and all(ap.get(k) == v for k, v in original_selectors.items())):
                matches.append(action)
        require(len(matches) == 1, "missing or ambiguous matching Ready byte continuation")
        require(len(actions) == 1 and page["recommended_action"] == 0, "nonterminal byte page must recommend its sole continuation")
        current = ready_arguments(matches[0])
        cp = current["params"]
        require(type(cp.get("limit_bytes")) is int and 1 <= cp["limit_bytes"] <= 4096, "invalid continuation byte limit")
        required_keys = set(original_selectors) | {"offset_bytes", "representation_digest", "limit_bytes"}
        if route == "scope.candidates.context":
            required_keys.add("candidate_set_revision")
            # The initial public Ready omits after; production dispatch makes
            # its logical zero default explicit in the actual byte continuation.
            if "after" not in original_selectors:
                require(type(cp.get("after")) is int and cp["after"] == 0, "candidate byte continuation changed default collection cursor")
                required_keys.add("after")
        require(set(cp) == required_keys, "byte continuation introduced unadvertised selectors")
        require(cp[identity] == source[identity], "continuation identity pin mismatch")
        if route == "scope.candidates.context":
            require(cp.get("candidate_set_revision") == source["candidate_set_revision"], "continuation revision pin mismatch")
        selectors = {k: v for k, v in cp.items() if k != "offset_bytes"}
        if continuation_selectors is None:
            continuation_selectors = selectors
        else:
            require(selectors == continuation_selectors, "byte continuation selectors changed")
    raise AssertionError("planning fragment page budget exceeded")


def read_ready(call, action):
    args = ready_arguments(action)
    return read_query(call, args["route"], args["params"], advertised_action=action)


def destination(receipt, field, route, identity, view=None):
    action = receipt["field_destinations"][field]
    args = ready_arguments(action)
    require(args["route"] == route, "wrong field destination route")
    key = "candidate_set_id" if route == "scope.candidates.context" else "scope_id"
    require(args["params"].get(key) == identity, "wrong field destination identity")
    if view is not None:
        require(args["params"].get("view") == view, "wrong field destination view")
    return action


def candidate_page(call, receipt, field="context_snapshot_and_knowledge", view="overview"):
    require("context" not in receipt and "draft" not in receipt, "candidate mutation must remain compact")
    read = read_ready(call, destination(receipt, field, "scope.candidates.context", receipt["candidate_set"]["id"], view))
    page = read.value
    require(page.get("view") == view, "candidate read view mismatch")
    candidate_pins(read, receipt)
    return read


def candidate_pins(read, receipt):
    context = read.value["context"]
    for key in ("id", "revision"):
        require(context["candidate_set"][key] == receipt["candidate_set"][key], "candidate receipt set mismatch")
    for key in ("id", "sequence", "program_revision", "method"):
        expected = receipt["snapshot"][key]
        actual = context["snapshot"][key]
        if key == "method":
            require(all(actual[k] == expected[k] for k in ("id", "revision", "digest")), "candidate method pin mismatch")
        else:
            require(actual == expected, "candidate snapshot pin mismatch")
    if read.provenance["source"]:
        ps = read.provenance["source"]
        require(ps["candidate_set_revision"] == receipt["candidate_set"]["revision"]
                and ps["snapshot_id"] == receipt["snapshot"]["id"], "candidate source pin mismatch")

def planning_pins(read, scope_id, candidate_set, snapshot_id):
    value = read.value
    require(value["scope"]["id"] == scope_id, "planning scope mismatch")
    require(all(value["candidate_set"][k] == candidate_set[k] for k in ("id", "revision")), "planning set mismatch")
    require(value["snapshot"]["id"] == snapshot_id, "planning snapshot mismatch")
    if read.provenance["source"]:
        require(read.provenance["source"] == {"scope_id": scope_id, "candidate_set_id": candidate_set["id"],
                                               "snapshot_id": snapshot_id}, "planning source pin mismatch")


def open_scope_reads(call, receipt, disposition):
    require(receipt.get("disposition") == disposition and "created" not in receipt and "replay" not in receipt,
            "expected compact Scope open disposition")
    scope = receipt["scope"]
    scope_read = read_ready(call, destination(receipt, "scope", "scope.context", scope["id"]))
    planning_read = read_ready(call, destination(receipt, "planning", "slice.candidates.context", scope["id"], "details"))
    for key in ("id", "revision", "source_candidate_set_id", "source_candidate_id", "slice_candidate_set_id"):
        require(scope_read.value[key] == scope[key] == planning_read.value["scope"][key], "Scope receipt identity mismatch")
    if scope_read.provenance["source"]:
        require(scope_read.provenance["source"] == {"scope_id": scope["id"], "scope_revision": scope["revision"]}, "Scope source pin mismatch")
    require(scope["slice_candidate_set_id"] == receipt["planning"]["candidate_set"]["id"], "Scope planning linkage mismatch")
    planning_pins(planning_read, scope["id"], receipt["planning"]["candidate_set"], receipt["planning"]["snapshot_id"])
    return scope_read, planning_read


def slice_planning_details(call, receipt):
    require("scope" not in receipt and "draft" not in receipt, "Slice planning mutation must remain compact")
    read = read_ready(call, destination(receipt, "complete_planning_context", "slice.candidates.context", receipt["scope_id"], "details"))
    planning_pins(read, receipt["scope_id"], receipt["candidate_set"], receipt["snapshot"]["id"])
    require(read.value["snapshot"]["sequence"] == receipt["snapshot"]["sequence"], "planning sequence mismatch")
    return read
