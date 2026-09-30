"""Private fresh S05 source adapter. Files carry host credentials, never authority material.

Uses the existing authenticated v2 Unix wire directly. The service authenticates
registration, native session, workspace and current persisted source authority.
The owner process and private socket directory are the local trust boundary;
this does not authenticate arbitrary code executing as that owner.
"""
from __future__ import annotations

from dataclasses import asdict, dataclass, field
import json
import os
from pathlib import Path
import re
import socket
import stat
import time

from scripts.caller_host_routing import (CallerRoutingRejected, CallerRoutingRequest,
    CurrentHostSelection, TrustedCurrentSelectionPort, _digest, _identifier,
    _json, _shape, _validate_material)

MAX_FRAME_BYTES = 8 * 1024 * 1024
_SEAL = object()
_SOURCE_CALL_TIMEOUT = 55.0
_SOURCE_IO_TIMEOUT = 5.0


def _path(path: Path) -> None:
    raw = str(path)
    if not path.is_absolute() or os.path.normpath(raw) != raw:
        raise CallerRoutingRejected("private host path must be absolute and normalized")
    current = Path(path.anchor)
    for part in path.parts[1:]:
        current /= part
        if stat.S_ISLNK(current.lstat().st_mode):
            raise CallerRoutingRejected("private host path cannot follow symlinks")


def _identity(metadata):
    return metadata.st_dev, metadata.st_ino, metadata.st_uid


def _socket_identity(path: Path):
    _path(path)
    parent, node = path.parent.lstat(), path.lstat()
    if (not stat.S_ISDIR(parent.st_mode) or stat.S_IMODE(parent.st_mode) != 0o700
            or not stat.S_ISSOCK(node.st_mode) or stat.S_IMODE(node.st_mode) != 0o600
            or parent.st_uid != os.getuid() or node.st_uid != os.getuid()):
        raise CallerRoutingRejected("source socket must be owned and private")
    return _identity(parent), _identity(node)


def _decode(raw: bytes):
    def unique(pairs):
        result = {}
        for key, value in pairs:
            if key in result:
                raise CallerRoutingRejected("duplicate source response field")
            result[key] = value
        return result
    def deny_number(value):
        raise CallerRoutingRejected("source response has unsupported numbers")
    try:
        return json.loads(raw.decode("utf-8"), object_pairs_hook=unique,
                          parse_float=deny_number, parse_constant=deny_number)
    except (UnicodeError, ValueError, TypeError):
        raise CallerRoutingRejected("malformed source response") from None


def _auth(path: Path):
    _path(path)
    with path.open("rb") as stream:
        opened = os.fstat(stream.fileno())
        raw = stream.read(4097)
        current = path.lstat()
    if (not stat.S_ISREG(opened.st_mode) or stat.S_IMODE(opened.st_mode) != 0o600
            or opened.st_uid != os.getuid() or opened.st_size > 4096 or len(raw) > 4096
            or _identity(opened) != _identity(current) or opened.st_size != current.st_size
            or opened.st_mtime_ns != current.st_mtime_ns or opened.st_ctime_ns != current.st_ctime_ns):
        raise CallerRoutingRejected("host credential file must be stable and private")
    auth = _shape(_decode(raw), "host_id credential")
    _identifier(auth["host_id"], "host ID")
    if not isinstance(auth["credential"], str) or re.fullmatch(r"[0-9a-fA-F]{64}", auth["credential"]) is None:
        raise CallerRoutingRejected("invalid host credential shape")
    return auth


@dataclass(frozen=True)
class _HostAdapterContext:
    socket_path: Path
    config_path: Path
    workspace_key: str
    native_session_id: str
    workspace_id: str
    actor_id: str
    session_id: str
    _seal: object = field(repr=False)


def _host_adapter_context(*, socket_path: Path, config_path: Path, workspace_key: str,
                          native_session_id: str, workspace_id: str, actor_id: str,
                          session_id: str) -> _HostAdapterContext:
    """Owner bootstrap with UNTRUSTED expected identity data pins.

    Auth/socket/native context establishes the direct service channel; expected
    UUIDs merely deny a foreign result and never grant authority themselves.
    Native session selector and persisted session UUID remain distinct.
    """
    for value in (native_session_id, workspace_id, actor_id, session_id):
        _identifier(value, "private host identity")
    if not isinstance(workspace_key, str) or re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9._-]{0,127}", workspace_key) is None:
        raise CallerRoutingRejected("invalid workspace key")
    _socket_identity(socket_path)
    _auth(config_path)
    return _HostAdapterContext(socket_path, config_path, workspace_key, native_session_id,
                               workspace_id, actor_id, session_id, _SEAL)


class AuthenticatedCurrentSource(TrustedCurrentSelectionPort):
    def __init__(self, context: _HostAdapterContext, *, output_capacity: int = MAX_FRAME_BYTES):
        if type(context) is not _HostAdapterContext or context._seal is not _SEAL:
            raise CallerRoutingRejected("private host adapter context required")
        if type(output_capacity) is not int or not 1 <= output_capacity <= MAX_FRAME_BYTES:
            raise CallerRoutingRejected("source output capacity outside wire bound")
        self._context, self._capacity = context, output_capacity

    def resolve_current(self, request: CallerRoutingRequest, *, input_sha256: str) -> CurrentHostSelection:
        deadline = time.monotonic() + _SOURCE_CALL_TIMEOUT
        def remaining(*, io_stage=False):
            seconds = deadline - time.monotonic()
            if seconds <= 0:
                raise CallerRoutingRejected("source call deadline exceeded")
            return min(_SOURCE_IO_TIMEOUT, seconds) if io_stage else seconds
        if type(request) is not CallerRoutingRequest:
            raise CallerRoutingRejected("exact immutable caller pins required")
        _digest(input_sha256, "task input digest")
        c = self._context
        expected = _socket_identity(c.socket_path)
        arguments = {**asdict(request), "input_sha256": input_sha256}
        wire = {"api_version": 2, "context": {"auth": _auth(c.config_path),
            "native_session_id": c.native_session_id, "workspace_key": c.workspace_key},
            "tool_name": "prepare_model_route_host_selection", "arguments": arguments,
            "output_capacity": self._capacity}
        encoded = (_json(wire) + "\n").encode("utf-8")
        if len(encoded) > MAX_FRAME_BYTES:
            raise CallerRoutingRejected("source request exceeds wire bound")
        try:
            with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as stream:
                stream.settimeout(remaining(io_stage=True))
                stream.connect(str(c.socket_path))
                if _socket_identity(c.socket_path) != expected:
                    raise CallerRoutingRejected("source socket identity changed")
                stream.settimeout(remaining(io_stage=True))
                stream.sendall(encoded)
                stream.settimeout(remaining(io_stage=True))
                stream.shutdown(socket.SHUT_WR)
                raw = bytearray()
                while True:
                    stream.settimeout(remaining())
                    part = stream.recv(min(65536, MAX_FRAME_BYTES + 1 - len(raw)))
                    if not part:
                        break
                    raw.extend(part)
                    if len(raw) > MAX_FRAME_BYTES:
                        raise CallerRoutingRejected("source response exceeds wire bound")
                    if b"\n" in raw and raw.index(b"\n") != len(raw) - 1:
                        raise CallerRoutingRejected("unexpected trailing source frame")
                if _socket_identity(c.socket_path) != expected:
                    raise CallerRoutingRejected("source socket identity changed")
        except (OSError, TimeoutError):
            raise CallerRoutingRejected("authenticated source unavailable without retry") from None
        if b"\n" not in raw:
            raise CallerRoutingRejected("source response missing frame")
        line, trailing = bytes(raw).split(b"\n", 1)
        if trailing:
            raise CallerRoutingRejected("unexpected trailing source frame")
        response = _decode(line)
        if not isinstance(response, dict) or response.get("status") != "ok":
            raise CallerRoutingRejected("authenticated source denied selection")
        _shape(response, "status result")
        result = _shape(response["result"], "material material_json material_sha256 authorization_scope actions recommended_action")
        if result["actions"] != [] or result["recommended_action"] is not None:
            raise CallerRoutingRejected("unexpected source selection actions")
        if len(_json(result).encode("utf-8")) > self._capacity:
            raise CallerRoutingRejected("source result exceeds requested capacity")
        if result["authorization_scope"] != "current_authenticated_read_only_snapshot":
            raise CallerRoutingRejected("source result lacks expected read-only scope")
        snapshot = CurrentHostSelection(result["material_json"], result["material_sha256"])
        material = _validate_material(snapshot, request, input_sha256)
        if _json(result["material"]) != snapshot.material_json:
            raise CallerRoutingRejected("source material projections differ")
        if (material["workspace_id"], material["invoking_actor_id"], material["invoking_session_id"]) != (c.workspace_id, c.actor_id, c.session_id):
            raise CallerRoutingRejected("source authenticated identities differ from expected data pins")
        remaining()
        return snapshot
