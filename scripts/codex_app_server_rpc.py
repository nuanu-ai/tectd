"""Owned stdio JSON-RPC transport for a caller-approved local Codex App Server.

This module is a transport primitive, not an authentication or dispatch policy.
The trusted composition root owns the exact local command and working directory.
It starts one child with private stdin/stdout/stderr pipes; it accepts no remote
endpoint, credential, arbitrary file reader, or model-route configuration. The
child uses the user's ordinary local Codex authentication. Never pass
untrusted command arguments here.
"""

from __future__ import annotations

import json
import copy
import math
import os
import selectors
import subprocess
import threading
import time
from collections.abc import Callable, Sequence
from pathlib import Path
from typing import Any


_MAX_FRAME_BYTES = 64 * 1024 * 1024
_STDERR_TAIL_BYTES = 16 * 1024
_CLOSE_GRACE_SECONDS = 0.25
_TERMINATE_GRACE_SECONDS = 0.5
_KILL_GRACE_SECONDS = 0.5


class AppServerProtocolError(RuntimeError):
    """The child produced malformed or unsupported JSON-RPC traffic."""


class AppServerRpcError(RuntimeError):
    """The server returned a JSON-RPC error response."""

    def __init__(self, method: str, code: int):
        self.method = method
        self.code = code
        # Do not include server-controlled error text or response contents.
        super().__init__(f"App Server RPC {method!r} failed with code {code}")


class OwnedAppServerRpc:
    """One-process JSON-RPC client over owned stdio pipes.

    Requests are never retried. A timed-out request burns and closes the transport.
    """

    def __init__(self, command: Sequence[str], cwd: str | os.PathLike[str]):
        if isinstance(command, (str, bytes)) or not isinstance(command, Sequence):
            raise TypeError("command must be a sequence of strings")
        argv = list(command)
        if not argv or any(not isinstance(part, str) for part in argv) or not argv[0]:
            raise ValueError("command must contain an executable string")
        working_directory = Path(cwd)
        if not working_directory.is_absolute():
            raise ValueError("cwd must be an absolute path")

        try:
            self._process = subprocess.Popen(
                argv,
                cwd=working_directory,
                stdin=subprocess.PIPE,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                bufsize=0,
            )
        except OSError as error:
            # argv may contain sensitive values; do not include it in the error.
            raise RuntimeError(f"could not start the local App Server child ({type(error).__name__})") from None

        # Popen guarantees these streams when the corresponding pipes are set.
        assert self._process.stdin is not None
        assert self._process.stdout is not None
        assert self._process.stderr is not None
        self._stdin = self._process.stdin
        self._stdout = self._process.stdout
        self._stderr = self._process.stderr
        os.set_blocking(self._stdin.fileno(), False)

        self._condition = threading.Condition()
        self._write_lock = threading.Lock()
        self._stop_readers = threading.Event()
        self._next_id = 0
        self._pending: dict[int, dict[str, Any] | None] = {}
        self._notifications: list[dict[str, Any]] = []
        self._all_notifications: list[dict[str, Any]] = []
        self._terminal_error: BaseException | None = None
        self._closed = False
        self._sealing = False
        self._seal_called = False
        self._stdout_eof = False
        self._stderr_tail = bytearray()

        self._stdout_thread = threading.Thread(
            target=self._read_stdout, name="codex-app-server-stdout", daemon=True
        )
        self._stderr_thread = threading.Thread(
            target=self._drain_stderr, name="codex-app-server-stderr", daemon=True
        )
        self._stdout_thread.start()
        self._stderr_thread.start()

    def __enter__(self) -> OwnedAppServerRpc:
        return self

    def __exit__(self, exc_type: Any, exc: Any, traceback: Any) -> None:
        self.close()

    def request(self, method: str, params: dict[str, Any], timeout: float = 30.0) -> dict[str, Any]:
        """Send one request and return its matched result object."""
        if not isinstance(method, str) or not method:
            raise ValueError("method must be a nonempty string")
        if not isinstance(params, dict):
            raise TypeError("params must be a JSON object")
        timeout = self._timeout(timeout)
        deadline = time.monotonic() + timeout

        with self._condition:
            self._raise_if_unavailable()
            self._next_id += 1
            request_id = self._next_id
            self._pending[request_id] = None

        message = {"id": request_id, "method": method, "params": params}
        try:
            encoded = self._encode(message)
            self._write(encoded, deadline)
            return self._await_response(request_id, method, deadline)
        except TimeoutError as error:
            self._set_terminal(error)
            self.close()
            raise
        finally:
            with self._condition:
                self._pending.pop(request_id, None)

    def initialize(self, client_info: dict[str, Any], timeout: float = 30.0) -> dict[str, Any]:
        """Perform the App Server initialize request and initialized notice."""
        if not isinstance(client_info, dict):
            raise TypeError("client_info must be a JSON object")
        deadline = time.monotonic() + self._timeout(timeout)
        result = self.request("initialize", {"clientInfo": dict(client_info)}, timeout=timeout)
        try:
            self._write(self._encode({"method": "initialized", "params": {}}), deadline)
        except TimeoutError as error:
            self._set_terminal(error)
            self.close()
            raise
        return result

    def seal_notifications(self, timeout: float = 30.0) -> list[dict[str, Any]]:
        """Single-use complete snapshot, including previously consumed notices.

        Close stdin, require clean child exit and stdout EOF/reader completion,
        then return all owned notifications. No requests are allowed once sealing
        starts. Incomplete or forced closure raises; it never returns a snapshot.
        """
        deadline = time.monotonic() + self._timeout(timeout)
        with self._condition:
            if self._seal_called or self._closed:
                raise RuntimeError("App Server notification capture cannot be sealed again")
            if self._pending:
                raise RuntimeError("cannot seal App Server capture with requests pending")
            self._seal_called = True
            self._sealing = True
            self._condition.notify_all()
        try:
            with self._condition:
                if self._terminal_error is not None and not isinstance(self._terminal_error, EOFError):
                    raise AppServerProtocolError("App Server notification capture is incomplete")
            if not self._close_stdin(deadline):
                raise AppServerProtocolError("App Server notification capture closure timed out")
            if not self._wait_for_child(max(0.0, deadline - time.monotonic())):
                raise AppServerProtocolError("App Server notification capture closure timed out")
            self._stdout_thread.join(timeout=max(0.0, deadline - time.monotonic()))
            self._stderr_thread.join(timeout=max(0.0, deadline - time.monotonic()))
            with self._condition:
                if (
                    self._closed or self._stop_readers.is_set()
                    or self._process.returncode != 0 or not self._stdout_eof
                    or self._stdout_thread.is_alive() or self._stderr_thread.is_alive()
                    or (self._terminal_error is not None and not isinstance(self._terminal_error, EOFError))
                ):
                    raise AppServerProtocolError("App Server notification capture is incomplete")
                snapshot = copy.deepcopy(self._all_notifications)
            self.close()
            return snapshot
        except BaseException:
            self.close()
            raise

    def diagnostics(self) -> dict[str, Any]:
        """Read owned child metadata and a bounded, potentially sensitive tail.

        This does not log or echo stderr. The caller must redact any diagnostics
        before sharing them; this accessor is not evidence of a complete capture.
        """
        with self._condition:
            return {
                "exit_code": self._process.poll(),
                "stderr_tail": bytes(self._stderr_tail).decode("utf-8", errors="replace"),
                "stdout_eof": self._stdout_eof,
            }

    def take_notifications(self, method: str | None = None) -> list[dict[str, Any]]:
        """Remove and return captured notifications, optionally for one method."""
        if method is not None and (not isinstance(method, str) or not method):
            raise ValueError("method must be a nonempty string or None")
        with self._condition:
            if method is None:
                notifications = self._notifications
                self._notifications = []
                return notifications
            matched = [item for item in self._notifications if item.get("method") == method]
            self._notifications = [item for item in self._notifications if item.get("method") != method]
            return matched

    def wait_notification(
        self,
        method: str,
        predicate: Callable[[dict[str, Any]], bool],
        timeout: float,
    ) -> dict[str, Any]:
        """Wait for and remove the first matching notification.

        Notifications that do not match remain available through this method or
        :meth:`take_notifications`.
        """
        if not isinstance(method, str) or not method:
            raise ValueError("method must be a nonempty string")
        if not callable(predicate):
            raise TypeError("predicate must be callable")
        timeout = self._timeout(timeout)
        deadline = time.monotonic() + timeout
        with self._condition:
            while True:
                for index, notification in enumerate(self._notifications):
                    if notification.get("method") == method and predicate(notification):
                        return self._notifications.pop(index)
                self._raise_if_unavailable()
                remaining = deadline - time.monotonic()
                if remaining <= 0:
                    raise TimeoutError(f"timed out waiting for App Server notification {method!r}")
                self._condition.wait(remaining)

    def close(self) -> None:
        """Close only this owned child, using bounded graceful/terminate/kill waits."""
        with self._condition:
            if self._closed:
                return
            self._closed = True
            self._condition.notify_all()

        # The nonblocking writer observes _closed; never wait unbounded for it.
        self._close_stdin(time.monotonic() + _CLOSE_GRACE_SECONDS)

        exited = self._wait_for_child(_CLOSE_GRACE_SECONDS)
        if not exited:
            try:
                self._process.terminate()
            except (OSError, ValueError):
                pass
            exited = self._wait_for_child(_TERMINATE_GRACE_SECONDS)
        if not exited:
            try:
                self._process.kill()
            except OSError:
                pass
            self._wait_for_child(_KILL_GRACE_SECONDS)
        self._close_stdin(time.monotonic() + _CLOSE_GRACE_SECONDS)

        # Give exited-child readers a bounded opportunity to drain final bytes.
        self._stdout_thread.join(timeout=_CLOSE_GRACE_SECONDS)
        self._stderr_thread.join(timeout=_CLOSE_GRACE_SECONDS)
        self._stop_readers.set()
        self._stdout_thread.join(timeout=0.5)
        self._stderr_thread.join(timeout=0.5)
        for stream in (self._stdout, self._stderr):
            try:
                stream.close()
            except OSError:
                pass

    @staticmethod
    def _timeout(timeout: float) -> float:
        if isinstance(timeout, bool) or not isinstance(timeout, (int, float)):
            raise TypeError("timeout must be a finite nonnegative number")
        if not math.isfinite(timeout) or timeout < 0:
            raise ValueError("timeout must be a finite nonnegative number")
        return float(timeout)

    @staticmethod
    def _encode(message: dict[str, Any]) -> bytes:
        try:
            return (json.dumps(message, ensure_ascii=False, allow_nan=False, separators=(",", ":")) + "\n").encode(
                "utf-8"
            )
        except (TypeError, ValueError, UnicodeError):
            raise AppServerProtocolError("outgoing JSON-RPC message is not valid JSON") from None

    def _close_stdin(self, deadline: float) -> bool:
        if not self._write_lock.acquire(timeout=max(0.0, deadline - time.monotonic())):
            return False
        try:
            self._stdin.close()
            return True
        except (OSError, ValueError):
            return True
        finally:
            self._write_lock.release()

    def _write(self, payload: bytes, deadline: float) -> None:
        if not self._write_lock.acquire(timeout=max(0.0, deadline - time.monotonic())):
            raise TimeoutError("App Server RPC write timed out")
        selector = selectors.DefaultSelector()
        try:
            with self._condition:
                self._raise_if_unavailable()
            view = memoryview(payload)
            try:
                descriptor = self._stdin.fileno()
                selector.register(descriptor, selectors.EVENT_WRITE)
                while view:
                    with self._condition:
                        self._raise_if_unavailable()
                    remaining = deadline - time.monotonic()
                    if remaining <= 0:
                        raise TimeoutError("App Server RPC write timed out")
                    if not selector.select(timeout=min(remaining, 0.05)):
                        continue
                    try:
                        written = os.write(descriptor, view)
                    except BlockingIOError:
                        continue
                    if written <= 0:
                        raise OSError("child input closed")
                    view = view[written:]
            except TimeoutError:
                raise
            except (OSError, ValueError):
                error = AppServerProtocolError("could not write to the local App Server child")
                self._set_terminal(error)
                raise error from None
        finally:
            selector.close()
            self._write_lock.release()

    def _await_response(self, request_id: int, method: str, deadline: float) -> dict[str, Any]:
        with self._condition:
            while True:
                if deadline - time.monotonic() <= 0:
                    raise TimeoutError(f"App Server RPC {method!r} timed out")
                response = self._pending.get(request_id)
                if response is not None:
                    if "error" in response:
                        raise AppServerRpcError(method, response["error"]["code"])
                    result = response["result"]
                    if not isinstance(result, dict):
                        raise AppServerProtocolError("App Server RPC result is not an object")
                    return result
                self._raise_if_unavailable()
                remaining = deadline - time.monotonic()
                if remaining <= 0:
                    raise TimeoutError(f"App Server RPC {method!r} timed out")
                self._condition.wait(remaining)

    def _raise_if_unavailable(self) -> None:
        if self._closed:
            raise RuntimeError("App Server transport is closed")
        if self._sealing:
            raise RuntimeError("App Server notification capture is sealing")
        if self._terminal_error is not None:
            raise self._terminal_error

    def _set_terminal(self, error: BaseException) -> None:
        with self._condition:
            if self._terminal_error is None or isinstance(self._terminal_error, EOFError):
                self._terminal_error = error
            self._condition.notify_all()

    def _read_stdout(self) -> None:
        buffer = bytearray()
        selector = selectors.DefaultSelector()
        try:
            selector.register(self._stdout.fileno(), selectors.EVENT_READ)
            while not self._stop_readers.is_set():
                if not selector.select(timeout=0.1):
                    continue
                chunk = os.read(self._stdout.fileno(), 8192)
                if not chunk:
                    if buffer:
                        raise AppServerProtocolError("App Server closed stdout with an incomplete frame")
                    with self._condition:
                        self._stdout_eof = True
                    return
                buffer.extend(chunk)
                while True:
                    boundary = buffer.find(b"\n")
                    if boundary < 0:
                        if len(buffer) > _MAX_FRAME_BYTES:
                            raise AppServerProtocolError("App Server JSON-RPC frame exceeds the size limit")
                        break
                    if boundary > _MAX_FRAME_BYTES:
                        raise AppServerProtocolError("App Server JSON-RPC frame exceeds the size limit")
                    line = bytes(buffer[:boundary])
                    del buffer[: boundary + 1]
                    if line.endswith(b"\r"):
                        line = line[:-1]
                    self._dispatch_frame(line)
        except AppServerProtocolError as error:
            self._set_terminal(error)
        except (OSError, UnicodeError, ValueError, RecursionError):
            self._set_terminal(AppServerProtocolError("could not read App Server JSON-RPC traffic"))
        finally:
            selector.close()
            with self._condition:
                if self._terminal_error is None and not self._closed and not self._sealing and not self._stop_readers.is_set():
                    self._terminal_error = EOFError("App Server child closed stdout")
                self._condition.notify_all()

    def _dispatch_frame(self, raw: bytes) -> None:
        def reject_constant(_: str) -> Any:
            raise ValueError("non-JSON numeric constant")

        try:
            frame = json.loads(raw.decode("utf-8"), parse_constant=reject_constant)
        except (UnicodeError, ValueError, RecursionError):
            raise AppServerProtocolError("App Server emitted malformed JSON-RPC data") from None
        # Codex App Server omits the JSON-RPC version field on its wire.
        # Retain compatibility with an explicit valid version, but reject others.
        if not isinstance(frame, dict) or ("jsonrpc" in frame and frame["jsonrpc"] != "2.0"):
            raise AppServerProtocolError("App Server emitted a malformed JSON-RPC object")

        has_id = "id" in frame
        has_method = "method" in frame
        if has_id and has_method:
            raise AppServerProtocolError("App Server requests are unsupported by this transport")
        if has_id:
            request_id = frame["id"]
            if isinstance(request_id, bool) or not isinstance(request_id, int):
                raise AppServerProtocolError("App Server response has an invalid request ID")
            has_result = "result" in frame
            has_error = "error" in frame
            if has_result == has_error:
                raise AppServerProtocolError("App Server response must contain exactly one of result or error")
            if has_result and not isinstance(frame["result"], dict):
                raise AppServerProtocolError("App Server RPC result is not an object")
            if has_error:
                error = frame["error"]
                if (
                    not isinstance(error, dict)
                    or isinstance(error.get("code"), bool)
                    or not isinstance(error.get("code"), int)
                    or not isinstance(error.get("message"), str)
                ):
                    raise AppServerProtocolError("App Server returned a malformed JSON-RPC error")
            with self._condition:
                if request_id in self._pending:
                    self._pending[request_id] = frame
                # Unknown IDs are stale or unrelated replies; never match them.
                self._condition.notify_all()
            return

        if has_method:
            if not isinstance(frame["method"], str) or not frame["method"]:
                raise AppServerProtocolError("App Server notification has an invalid method")
            with self._condition:
                self._notifications.append(frame)
                self._all_notifications.append(copy.deepcopy(frame))
                self._condition.notify_all()
            return
        raise AppServerProtocolError("App Server emitted an object without a method or response ID")

    def _drain_stderr(self) -> None:
        """Drain stderr, retaining only a bounded tail without logging it."""
        selector = selectors.DefaultSelector()
        try:
            selector.register(self._stderr.fileno(), selectors.EVENT_READ)
            while not self._stop_readers.is_set():
                if not selector.select(timeout=0.1):
                    continue
                chunk = os.read(self._stderr.fileno(), 8192)
                if not chunk:
                    return
                with self._condition:
                    self._stderr_tail.extend(chunk)
                    if len(self._stderr_tail) > _STDERR_TAIL_BYTES:
                        del self._stderr_tail[:-_STDERR_TAIL_BYTES]
        except OSError:
            return
        finally:
            selector.close()

    def _wait_for_child(self, timeout: float) -> bool:
        try:
            self._process.wait(timeout=timeout)
            return True
        except subprocess.TimeoutExpired:
            return False
