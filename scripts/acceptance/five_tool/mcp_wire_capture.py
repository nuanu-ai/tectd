#!/usr/bin/env python3
"""Private byte-exact MCP relay and evidence reader for native acceptance."""
from __future__ import annotations

import argparse
import base64
import fcntl
import hashlib
import json
import os
import pathlib
import subprocess
import sys
import threading
import time
import uuid
from typing import Any

PUBLIC_TOOLS = {"get_state", "query", "command", "execute", "help"}
QUERY_ROUTES = {"scope.candidates.context"}
COMMAND_ROUTES = {
    "workspace.open",
    "scope.candidates.begin", "scope.candidates.save",
    "scope.candidates.record_input", "scope.candidates.refresh",
}


def sha256_file(path: pathlib.Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def write_private_json(path: pathlib.Path, value: dict[str, Any]) -> None:
    temporary = path.with_suffix(path.suffix + ".tmp")
    descriptor = os.open(temporary, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600)
    with os.fdopen(descriptor, "w") as stream:
        json.dump(value, stream, sort_keys=True)
        stream.write("\n")
    temporary.replace(path)


def read_json(path: pathlib.Path) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text())
        return value if isinstance(value, dict) else {}
    except (FileNotFoundError, json.JSONDecodeError, OSError):
        return {}


def native_id(value: Any) -> str | None:
    if not isinstance(value, str):
        return None
    try:
        parsed = uuid.UUID(value)
    except ValueError:
        return None
    return value if parsed.int and str(parsed) == value else None


def guarded_error(request: Any, policy: dict[str, Any], gate: dict[str, Any]) -> str | None:
    if not isinstance(request, dict) or request.get("method") != "tools/call":
        return None
    params = request.get("params")
    if not isinstance(params, dict):
        return "invalid_tools_call"
    tool = params.get("name")
    if tool not in PUBLIC_TOOLS:
        return "forbidden_tool"
    metadata = params.get("_meta")
    actor = native_id(metadata.get("threadId") if isinstance(metadata, dict) else None)
    if actor is None:
        return "invalid_native_actor"
    parent = policy.get("parent_thread_id")
    if actor == parent:
        return "parent_mcp_forbidden"
    arguments = params.get("arguments", {})
    if not isinstance(arguments, dict):
        return "invalid_arguments"
    if tool == "execute":
        return "execute_forbidden"
    if tool == "get_state" and arguments:
        return "invalid_get_state_arguments"
    if tool in {"query", "command"}:
        if set(arguments) != {"route", "params"} or not isinstance(arguments.get("params"), dict):
            return "invalid_routed_envelope"
        routes = QUERY_ROUTES if tool == "query" else COMMAND_ROUTES
        if arguments.get("route") not in routes:
            return "forbidden_route"
    if gate:
        if gate.get("parent_thread_id") != parent or gate.get("child_thread_id") != actor:
            return "actor_gate_mismatch"
    elif tool == "command":
        return "wait_for_identity_gate"
    return None


class PrivateLog:
    def __init__(self, directory: pathlib.Path, chain: dict[str, Any], phase: str = "transparent"):
        directory.mkdir(mode=0o700, parents=True, exist_ok=True)
        os.chmod(directory, 0o700)
        self.connection_id = str(uuid.uuid4())
        self.path = directory / f"mcp-wire-{self.connection_id}.jsonl"
        self.counter_path = directory / "global-sequence"
        descriptor = os.open(self.path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
        self.stream = os.fdopen(descriptor, "wb", buffering=0)
        self.lock = threading.Lock()
        self.sequence = 0
        self.pending: dict[str, int] = {}
        self.append("connection", b"", chain, phase, None)

    def global_sequence(self) -> int:
        descriptor = os.open(self.counter_path, os.O_RDWR | os.O_CREAT, 0o600)
        with os.fdopen(descriptor, "r+") as stream:
            fcntl.flock(stream, fcntl.LOCK_EX)
            value = stream.read().strip()
            sequence = int(value) + 1 if value else 1
            stream.seek(0)
            stream.truncate()
            stream.write(str(sequence) + "\n")
            stream.flush()
            os.fsync(stream.fileno())
            fcntl.flock(stream, fcntl.LOCK_UN)
        return sequence

    @staticmethod
    def key(value: Any) -> str:
        return json.dumps(value, sort_keys=True, separators=(",", ":"))

    def _append_locked(
        self, direction: str, raw: bytes, parsed: Any, phase: str, request_sequence: int | None,
        response_source: str | None = None, forwarded: bool | None = None,
    ) -> int:
        self.sequence += 1
        record = {
            "sequence": self.sequence, "connection_id": self.connection_id,
            "global_sequence": self.global_sequence(), "direction": direction, "phase": phase,
            "raw_base64": base64.b64encode(raw).decode(),
            "raw_sha256": hashlib.sha256(raw).hexdigest(), "parsed": parsed,
            "request_sequence": request_sequence, "response_source": response_source,
            "forwarded": forwarded,
        }
        encoded = json.dumps(record, ensure_ascii=False, separators=(",", ":")).encode() + b"\n"
        self.stream.write(encoded)
        os.fsync(self.stream.fileno())
        return self.sequence

    def append(
        self, direction: str, raw: bytes, parsed: Any, phase: str, request_sequence: int | None,
        response_source: str | None = None, forwarded: bool | None = None,
    ) -> int:
        with self.lock:
            return self._append_locked(
                direction, raw, parsed, phase, request_sequence, response_source, forwarded,
            )

    def request(self, raw: bytes, parsed: Any, phase: str) -> tuple[int, bool]:
        with self.lock:
            key = self.key(parsed["id"]) if isinstance(parsed, dict) and "id" in parsed else None
            duplicate = key is not None and key in self.pending
            sequence = self._append_locked("request", raw, parsed, phase, None)
            if key is not None and not duplicate:
                self.pending[key] = sequence
            return sequence, duplicate

    def response(
        self, raw: bytes, parsed: Any, phase: str, *, response_source: str,
        forwarded: bool, request_sequence: int | None = None,
    ) -> int:
        with self.lock:
            if request_sequence is None and isinstance(parsed, dict) and "id" in parsed:
                request_sequence = self.pending.pop(self.key(parsed["id"]), None)
            return self._append_locked(
                "response", raw, parsed, phase, request_sequence, response_source, forwarded,
            )

    def close(self) -> None:
        self.stream.close()


def parse(raw: bytes) -> Any:
    try:
        return json.loads(raw)
    except (UnicodeDecodeError, json.JSONDecodeError):
        return {"malformed": True}


def relay(
    *, log_dir: pathlib.Path, policy_path: pathlib.Path, gate_path: pathlib.Path,
    launcher: pathlib.Path, relay_path: pathlib.Path, run_sh: pathlib.Path,
    binary: pathlib.Path, gate_wait_seconds: float = 15.0,
) -> int:
    chain = {
        "target_argv": ["/bin/sh", "./run.sh"],
        "launcher": {"path": str(launcher), "sha256": sha256_file(launcher)},
        "relay": {"path": str(relay_path), "sha256": sha256_file(relay_path)},
        "run_sh": {"path": str(run_sh), "sha256": sha256_file(run_sh)},
        "packaged_binary": {"path": str(binary), "sha256": sha256_file(binary)},
    }
    initial_phase = str(read_json(policy_path).get("phase", "transparent"))
    log = PrivateLog(log_dir, chain, initial_phase)
    child = subprocess.Popen(
        ["/bin/sh", "./run.sh"], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
        stderr=None, cwd=run_sh.parent,
    )
    output_lock = threading.Lock()

    def write_client(
        raw: bytes, parsed: Any, phase: str, *, response_source: str,
        forwarded: bool, request_sequence: int | None = None,
    ) -> None:
        log.response(
            raw, parsed, phase, response_source=response_source, forwarded=forwarded,
            request_sequence=request_sequence,
        )
        with output_lock:
            sys.stdout.buffer.write(raw)
            sys.stdout.buffer.flush()

    def output() -> None:
        assert child.stdout is not None
        for raw in iter(child.stdout.readline, b""):
            policy = read_json(policy_path)
            write_client(
                raw, parse(raw), str(policy.get("phase", "transparent")),
                response_source="mcp_wire", forwarded=True,
            )

    pump = threading.Thread(target=output, name="tectd-mcp-relay-output", daemon=True)
    pump.start()
    try:
        assert child.stdin is not None
        for raw in iter(sys.stdin.buffer.readline, b""):
            request = parse(raw)
            policy = read_json(policy_path)
            phase = str(policy.get("phase", "transparent"))
            sequence, duplicate = log.request(raw, request, phase)
            error = "duplicate_request_id" if duplicate else None
            if phase == "guarded" and error is None:
                gate = read_json(gate_path)
                error = guarded_error(request, policy, gate)
                if error == "wait_for_identity_gate":
                    deadline = time.monotonic() + gate_wait_seconds
                    while time.monotonic() < deadline and error == "wait_for_identity_gate":
                        time.sleep(0.05)
                        gate = read_json(gate_path)
                        error = guarded_error(request, policy, gate)
            if error is not None:
                identifier = request.get("id") if isinstance(request, dict) else None
                response = {"jsonrpc": "2.0", "id": identifier,
                            "error": {"code": -32600, "message": "native_capture_" + error}}
                encoded = json.dumps(response, separators=(",", ":")).encode() + b"\n"
                write_client(
                    encoded, response, phase, response_source="fixture_capture", forwarded=False,
                    request_sequence=sequence if duplicate else None,
                )
                continue
            child.stdin.write(raw)
            child.stdin.flush()
        child.stdin.close()
        pump.join(timeout=5)
        return child.wait(timeout=5)
    finally:
        if child.poll() is None:
            child.terminate()
            try:
                child.wait(timeout=5)
            except subprocess.TimeoutExpired:
                child.kill()
                child.wait(timeout=5)
        log.close()


def read_records(directory: pathlib.Path, require_contiguous: bool = True) -> list[dict[str, Any]]:
    records: list[dict[str, Any]] = []
    for path in sorted(directory.glob("mcp-wire-*.jsonl")):
        if path.stat().st_mode & 0o077:
            raise AssertionError("MCP wire evidence is not private")
        prior = 0
        connection = None
        content = path.read_bytes()
        lines = content.splitlines(keepends=True)
        if lines and not lines[-1].endswith(b"\n"):
            if require_contiguous:
                raise AssertionError("MCP wire log ends with a partial record")
            lines.pop()
        for raw_line in lines:
            row = json.loads(raw_line)
            if row.get("direction") not in {"connection", "request", "response"}:
                raise AssertionError("MCP wire record has an invalid direction")
            if row.get("phase") not in {"transparent", "guarded"}:
                raise AssertionError("MCP wire record has an invalid phase")
            if row["direction"] == "response":
                expected = {"mcp_wire": True, "fixture_capture": False}
                if (row.get("response_source") not in expected
                        or expected[row["response_source"]] is not row.get("forwarded")):
                    raise AssertionError("MCP wire response origin is inconsistent")
            elif row.get("response_source") is not None or row.get("forwarded") is not None:
                raise AssertionError("non-response MCP wire record has response origin fields")
            if row["sequence"] != prior + 1:
                raise AssertionError("MCP wire log sequence is not contiguous")
            prior = row["sequence"]
            connection = connection or row["connection_id"]
            if row["connection_id"] != connection:
                raise AssertionError("MCP wire log mixed connection identities")
            raw = base64.b64decode(row["raw_base64"], validate=True)
            if hashlib.sha256(raw).hexdigest() != row["raw_sha256"]:
                raise AssertionError("MCP wire raw frame digest mismatch")
            row["evidence_path"] = str(path)
            records.append(row)
    records.sort(key=lambda row: row["global_sequence"])
    if require_contiguous and [row["global_sequence"] for row in records] != list(range(1, len(records) + 1)):
        raise AssertionError("MCP wire global sequence is not contiguous")
    return records


def tool_pairs(
    directory: pathlib.Path, phase: str = "guarded", allow_pending: bool = False,
) -> tuple[list[dict[str, Any]], list[dict[str, Any]]]:
    records = read_records(directory, require_contiguous=not allow_pending)
    relevant = [row for row in records if row["phase"] == phase]
    errors = [{"error": "malformed MCP frame", "record": row}
              for row in relevant if row["parsed"] == {"malformed": True}]
    all_requests = {(row["connection_id"], row["sequence"]): row for row in records
                    if row["direction"] == "request"}
    responses: dict[tuple[str, int], dict[str, Any]] = {}
    for row in relevant:
        if row["direction"] != "response":
            continue
        if isinstance(row["parsed"], dict) and "id" not in row["parsed"] \
                and isinstance(row["parsed"].get("method"), str):
            continue
        key = (row["connection_id"], row.get("request_sequence"))
        request = all_requests.get(key)
        if request is None:
            errors.append({"error": "MCP response has no same-connection request", "response": row["parsed"]})
            continue
        if (row["sequence"] <= request["sequence"]
                or row["global_sequence"] <= request["global_sequence"]):
            errors.append({"error": "MCP response precedes its request", "response": row["parsed"]})
            continue
        if row["phase"] != request["phase"]:
            errors.append({"error": "MCP response phase differs from its request", "response": row["parsed"]})
            continue
        if key in responses:
            errors.append({"error": "duplicate MCP response", "response": row["parsed"]})
            continue
        responses[key] = row
    requests = {(row["connection_id"], row["sequence"]): row for row in relevant
                if row["direction"] == "request" and row["phase"] == phase
                and isinstance(row["parsed"], dict) and row["parsed"].get("method") == "tools/call"}
    pairs = []
    for key, request in sorted(requests.items(), key=lambda item: item[1]["global_sequence"]):
        response = responses.get(key)
        if response is None:
            if not allow_pending:
                errors.append({"error": "unmatched MCP request", "request": request["parsed"]})
            continue
        pairs.append({
            "source": "mcp_wire", "connection_id": request["connection_id"],
            "response_source": response["response_source"], "forwarded": response["forwarded"],
            "request_sequence": key[1], "request": request["parsed"],
            "global_request_sequence": request["global_sequence"],
            "response": response["parsed"], "request_raw_sha256": request["raw_sha256"],
            "response_raw_sha256": response["raw_sha256"],
        })
    return pairs, errors


def main() -> None:
    parser = argparse.ArgumentParser()
    for name in ["log-dir", "policy", "gate", "launcher", "relay", "run-sh", "binary"]:
        parser.add_argument("--" + name, required=True, type=pathlib.Path)
    parser.add_argument("--gate-wait-seconds", type=float, default=15.0)
    args = parser.parse_args()
    raise SystemExit(relay(
        log_dir=args.log_dir, policy_path=args.policy, gate_path=args.gate,
        launcher=args.launcher, relay_path=args.relay, run_sh=args.run_sh, binary=args.binary,
        gate_wait_seconds=args.gate_wait_seconds,
    ))


if __name__ == "__main__":
    main()
