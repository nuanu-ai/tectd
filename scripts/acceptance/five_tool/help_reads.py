"""Explicit byte reads for the five DEFAULT Help route descriptions only."""
from __future__ import annotations
from copy import deepcopy
import hashlib
import json
import re

ALLOWED = {
    ("query", "knowledge.search"),
    ("query", "knowledge.context"),
    ("query", "knowledge.maintenance"),
    ("command", "knowledge.maintenance_observe"),
    ("command", "knowledge.maintenance_begin"),
}
MAX_BYTES = 8 * 1024 * 1024
MAX_PAGES = 8192


def require(condition, message):
    if not condition:
        raise AssertionError(message)


def describe(call, tool, route):
    """Keep ordinary (page, failed) results; follow only actual Help byte calls."""
    require((tool, route) in ALLOWED, "unsupported DEFAULT Help description")
    initial = {"mode": "describe", "tool": tool, "route": route}
    current = deepcopy(initial)
    expected_source = {"tool": "help", "selectors": initial}
    data = bytearray()
    digest = total = window = None
    for _ in range(MAX_PAGES):
        page, failed = call("help", current)
        if failed:
            return page, failed
        require(isinstance(page, dict), "Help result must be an object")
        if page.get("kind") != "fragment":
            require(not data and digest is None, "Help fragment changed to ordinary JSON")
            return page, failed
        require(page.get("format") == "json" and page.get("encoding") == "utf-8", "invalid Help fragment encoding")
        require(page.get("source") == expected_source, "Help fragment selector source mismatch")
        pd, pt = page.get("representation_digest"), page.get("total_bytes")
        require(isinstance(pd, str) and re.fullmatch(r"[a-f0-9]{64}", pd), "invalid Help representation digest")
        require(type(pt) is int and 0 <= pt <= MAX_BYTES, "Help representation exceeds byte budget")
        if digest is None:
            digest, total = pd, pt
        else:
            require((pd, pt) == (digest, total), "Help fragment digest or total changed")
        offset, count, text = page.get("offset_bytes"), page.get("returned_bytes"), page.get("text")
        require(type(offset) is int and offset == len(data), "noncontiguous Help byte fragment")
        require(type(count) is int and 0 <= count <= 4096 and isinstance(text, str), "invalid Help fragment byte count")
        encoded = text.encode("utf-8")
        require(count == len(encoded) and offset + count <= total, "Help UTF-8 count or bounds mismatch")
        data.extend(encoded)
        require("next_offset_bytes" in page, "missing Help fragment EOF field")
        next_offset = page["next_offset_bytes"]
        if next_offset is None:
            require(len(data) == total, "premature Help fragment EOF")
            require(page.get("actions") == [] and "recommended_action" in page
                    and page["recommended_action"] is None, "Help EOF must have actual empty actions and null recommendation")
            require(hashlib.sha256(data).hexdigest() == digest, "assembled Help representation digest mismatch")
            value = json.loads(data)
            require(isinstance(value, dict), "complete Help representation must be object")
            return value, failed
        require(type(next_offset) is int and count > 0 and next_offset == len(data), "Help fragment failed to advance")
        actions = page.get("actions")
        require(isinstance(actions, list) and len(actions) == 1
                and type(page.get("recommended_action")) is int and page["recommended_action"] == 0,
                "Help byte page must recommend its sole actual continuation")
        action = actions[0]
        require(isinstance(action, dict) and set(action) == {"kind", "tool", "arguments"}
                and action["kind"] == "ready_call" and action["tool"] == "help", "Help continuation must be Ready Help")
        args = action["arguments"]
        require(isinstance(args, dict) and set(args) == set(initial) | {"offset_bytes", "limit_bytes", "representation_digest"},
                "Help continuation argument shape changed")
        require(all(args[k] == v for k, v in initial.items())
                and type(args["offset_bytes"]) is int and args["offset_bytes"] == next_offset
                and args["representation_digest"] == digest, "Help continuation selectors or pins changed")
        require(type(args["limit_bytes"]) is int and 1 <= args["limit_bytes"] <= 4096, "invalid Help continuation window")
        if window is None:
            window = args["limit_bytes"]
        else:
            require(args["limit_bytes"] == window, "Help byte window changed")
        # Execute the exact advertised arguments, never manufacture a continuation.
        current = deepcopy(args)
    raise AssertionError("Help fragment page budget exceeded")
