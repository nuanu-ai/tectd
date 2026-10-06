"""Explicit complete slice.pipelines reads; raw envelope checks belong to wire capture."""
from __future__ import annotations
from copy import deepcopy
from dataclasses import dataclass
import hashlib
import json
import re

MAX_BYTES = 8 * 1024 * 1024
MAX_PAGES = 8192
ROUTE = "slice.pipelines"


def require(condition, message):
    if not condition:
        raise AssertionError(message)


@dataclass(frozen=True)
class CatalogRead:
    value: dict
    provenance: dict


def read(call, params=None):
    """Preserve default Full and assemble only exact advertised catalog byte calls."""
    initial = {} if params is None else deepcopy(params)
    require(isinstance(initial, dict) and set(initial) <= {"view"}, "invalid initial catalog selectors")
    view = initial.get("view", "full")
    require(isinstance(view, str) and view in {"full", "summary"}, "invalid initial catalog view")
    source = {"tool": "query", "route": ROUTE, "view": view}
    current = {"route": ROUTE, "params": deepcopy(initial)}
    initial_query = deepcopy(current)
    data = bytearray()
    digest = total = window = None
    for pages in range(1, MAX_PAGES + 1):
        page, failed = call("query", current)
        require(not failed and isinstance(page, dict), "catalog query failed")
        metadata = {key: deepcopy(page[key]) for key in ("actions", "recommended_action") if key in page}
        if page.get("kind") != "fragment":
            require(not data and digest is None, "catalog fragment changed to ordinary JSON")
            return CatalogRead(page, {"initial_query": initial_query, "source": None,
                                      "representation_digest": None, "pages": pages,
                                      "terminal_envelope_metadata": metadata})
        require(page.get("format") == "json" and page.get("encoding") == "utf-8", "invalid catalog fragment encoding")
        require(page.get("source") == source, "catalog canonical source or view changed")
        pd, pt = page.get("representation_digest"), page.get("total_bytes")
        require(isinstance(pd, str) and re.fullmatch(r"[a-f0-9]{64}", pd), "invalid catalog representation digest")
        require(type(pt) is int and 0 <= pt <= MAX_BYTES, "catalog representation exceeds byte budget")
        if digest is None:
            digest, total = pd, pt
        else:
            require((pd, pt) == (digest, total), "catalog representation digest or total changed")
        offset, count, text = page.get("offset_bytes"), page.get("returned_bytes"), page.get("text")
        require(type(offset) is int and offset == len(data), "noncontiguous catalog byte fragment")
        require(type(count) is int and 0 <= count <= 4096 and isinstance(text, str), "invalid catalog fragment byte count")
        encoded = text.encode("utf-8")
        require(count == len(encoded) and count <= (window or 4096) and offset + count <= total,
                "catalog UTF-8 count or byte bounds mismatch")
        data.extend(encoded)
        require("next_offset_bytes" in page, "missing catalog fragment EOF field")
        next_offset = page["next_offset_bytes"]
        if next_offset is None:
            require(len(data) == total, "premature catalog fragment EOF")
            require(page.get("actions") == [] and "recommended_action" in page
                    and page["recommended_action"] is None, "catalog EOF must have actual empty actions and null recommendation")
            require(hashlib.sha256(data).hexdigest() == digest, "assembled catalog representation digest mismatch")
            value = json.loads(data)
            require(isinstance(value, dict), "complete catalog representation must be object")
            return CatalogRead(value, {"initial_query": initial_query, "source": source,
                                       "representation_digest": digest, "pages": pages,
                                       "total_bytes": total, "terminal_envelope_metadata": metadata})
        require(type(next_offset) is int and count > 0 and next_offset == len(data), "catalog fragment failed to advance")
        actions = page.get("actions")
        require(isinstance(actions, list) and len(actions) == 1
                and type(page.get("recommended_action")) is int and page["recommended_action"] == 0,
                "catalog byte page must recommend its sole actual continuation")
        action = actions[0]
        require(isinstance(action, dict) and set(action) == {"kind", "tool", "arguments"}
                and action["kind"] == "ready_call" and action["tool"] == "query", "catalog continuation must be Ready query")
        args = action["arguments"]
        require(isinstance(args, dict) and set(args) == {"route", "params"} and args["route"] == ROUTE,
                "catalog continuation route changed")
        cp = args["params"]
        require(isinstance(cp, dict) and set(cp) == {"view", "offset_bytes", "limit_bytes", "representation_digest"},
                "catalog continuation selector shape changed")
        require(cp["view"] == view and type(cp["offset_bytes"]) is int and cp["offset_bytes"] == next_offset
                and cp["representation_digest"] == digest, "catalog continuation view, offset or digest changed")
        require(type(cp["limit_bytes"]) is int and 1 <= cp["limit_bytes"] <= 4096, "invalid catalog continuation window")
        if window is None:
            window = cp["limit_bytes"]
        else:
            require(cp["limit_bytes"] == window, "catalog byte window changed")
        # Use the actual advertised call, including its canonical explicit Full selector.
        current = deepcopy(args)
    raise AssertionError("catalog fragment page budget exceeded")
