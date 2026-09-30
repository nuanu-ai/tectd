"""Focused transport tests using only a tiny fake local JSON-RPC subprocess."""

import json
import subprocess
import sys
import tempfile
import threading
import time
import unittest
from pathlib import Path

from scripts.codex_app_server_rpc import (
    AppServerProtocolError,
    AppServerRpcError,
    OwnedAppServerRpc,
)


FAKE_SERVER = r'''
import json
import sys
import time

mode = sys.argv[1]
record_path = sys.argv[2] if len(sys.argv) > 2 else None

def record(message):
    if record_path:
        with open(record_path, "a", encoding="utf-8") as stream:
            stream.write(json.dumps(message, separators=(",", ":")) + "\n")

def emit(message):
    sys.stdout.write(json.dumps(message, separators=(",", ":")) + "\n")
    sys.stdout.flush()

if mode == "no-read":
    time.sleep(60)
    sys.exit(0)
if mode == "diagnostic":
    sys.stderr.write("prefix" + "x" * 20000 + "diagnostic-tail")
    sys.stderr.flush()
    sys.exit(7)

for raw in sys.stdin:
    request = json.loads(raw)
    record(request)
    if mode == "stream":
        sys.stderr.write("x" * 262144)
        sys.stderr.flush()
        emit({"jsonrpc": "2.0", "id": request["id"] + 1, "result": {"wrong": True}})
        emit({"jsonrpc": "2.0", "method": "server/unrelated", "params": {"n": 1}})
        emit({"jsonrpc": "2.0", "method": "server/ready", "params": {"ready": True}})
        emit({"jsonrpc": "2.0", "id": request["id"], "result": {"ok": True}})
        break
    if mode == "error":
        emit({"jsonrpc": "2.0", "id": request["id"],
              "error": {"code": -32600, "message": "server-controlled detail"}})
        break
    if mode == "eof":
        break
    if mode == "malformed":
        sys.stdout.write("{not-json}\n")
        sys.stdout.flush()
        break
    if mode == "slow":
        time.sleep(60)
        break
    if mode == "initialize":
        if request.get("method") == "initialize":
            emit({"jsonrpc": "2.0", "id": request["id"], "result": {"protocolVersion": "1"}})
        elif request.get("method") == "initialized":
            emit({"jsonrpc": "2.0", "method": "fake/initialized", "params": {"seen": True}})
            break
    if mode in ("seal", "seal-hang", "seal-partial", "seal-error"):
        emit({"jsonrpc": "2.0", "method": "server/earlier", "params": {}})
        emit({"jsonrpc": "2.0", "id": request["id"], "result": {"ok": True}})

if mode == "seal":
    emit({"jsonrpc": "2.0", "method": "model/rerouted", "params": {"late": True}})
elif mode == "seal-hang":
    time.sleep(60)
elif mode == "seal-partial":
    sys.stdout.write('{"jsonrpc":')
    sys.stdout.flush()
elif mode == "seal-error":
    sys.exit(1)
'''


class OwnedAppServerRpcTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.cwd = Path(self.temporary.name).resolve()
        self.transports = []
        self.addCleanup(self._close_transports)

    def _close_transports(self):
        for transport in self.transports:
            transport.close()

    def start(self, mode, record_path=None):
        command = [sys.executable, "-u", "-c", FAKE_SERVER, mode]
        if record_path is not None:
            command.append(str(record_path))
        transport = OwnedAppServerRpc(command, cwd=self.cwd)
        self.transports.append(transport)
        return transport

    def test_matched_response_ignores_other_ids_and_captures_notifications(self):
        transport = self.start("stream")
        result = transport.request("probe", {}, timeout=3)
        self.assertEqual(result, {"ok": True})
        ready = transport.wait_notification(
            "server/ready", lambda notification: notification["params"].get("ready") is True, timeout=1
        )
        self.assertEqual(ready["method"], "server/ready")
        self.assertEqual(transport.take_notifications("server/unrelated")[0]["params"], {"n": 1})
        self.assertEqual(transport.take_notifications(), [])

    def test_rpc_error_is_reported_without_echoing_server_error_text(self):
        record = self.cwd / "received.jsonl"
        transport = self.start("error", record)
        with self.assertRaises(AppServerRpcError) as caught:
            transport.request("probe", {}, timeout=2)
        self.assertEqual(caught.exception.code, -32600)
        self.assertNotIn("server-controlled detail", str(caught.exception))
        self.assertEqual(len(record.read_text(encoding="utf-8").splitlines()), 1)

    def test_eof_and_malformed_frame_fail_waiting_request(self):
        for mode, error_type in (("eof", EOFError), ("malformed", AppServerProtocolError)):
            with self.subTest(mode=mode):
                transport = self.start(mode)
                with self.assertRaises(error_type):
                    transport.request("probe", {}, timeout=2)

    def test_timeout_is_not_retried_and_close_reaps_only_owned_child(self):
        record = self.cwd / "received.jsonl"
        transport = self.start("slow", record)
        process = transport._process
        with self.assertRaises(TimeoutError):
            transport.request("probe", {}, timeout=0.05)
        self.assertEqual(len(record.read_text(encoding="utf-8").splitlines()), 1)
        transport.close()
        self.assertIsNotNone(process.poll())

    def test_initialize_sends_initialized_notification(self):
        record = self.cwd / "received.jsonl"
        transport = self.start("initialize", record)
        result = transport.initialize({"name": "transport-test", "version": "1"}, timeout=2)
        self.assertEqual(result, {"protocolVersion": "1"})
        acknowledgement = transport.wait_notification(
            "fake/initialized", lambda notification: notification["params"].get("seen") is True, timeout=2
        )
        self.assertEqual(acknowledgement["method"], "fake/initialized")
        messages = [json.loads(line) for line in record.read_text(encoding="utf-8").splitlines()]
        self.assertEqual(messages[0]["method"], "initialize")
        self.assertEqual(messages[0]["params"], {"clientInfo": {"name": "transport-test", "version": "1"}})
        self.assertEqual(messages[1], {"jsonrpc": "2.0", "method": "initialized"})

    def test_oversized_write_has_end_to_end_deadline_and_burns_transport(self):
        transport = self.start("no-read")
        started = time.monotonic()
        with self.assertRaises(TimeoutError):
            transport.request("probe", {"payload": "x" * (2 * 1024 * 1024)}, timeout=0.05)
        self.assertLess(time.monotonic() - started, 1.5)
        self.assertIsNotNone(transport._process.poll())
        with self.assertRaises(RuntimeError):
            transport.request("probe", {}, timeout=1)
        with self.assertRaises(RuntimeError):
            transport.seal_notifications(timeout=1)
        transport.close()

    def test_close_is_bounded_during_an_inflight_full_pipe_write(self):
        transport = self.start("no-read")
        failures = []

        def request():
            try:
                transport.request("probe", {"payload": "x" * (2 * 1024 * 1024)}, timeout=30)
            except BaseException as error:
                failures.append(error)

        writer = threading.Thread(target=request)
        writer.start()
        time.sleep(0.05)
        started = time.monotonic()
        transport.close()
        writer.join(timeout=1)
        self.assertLess(time.monotonic() - started, 1.5)
        self.assertFalse(writer.is_alive())
        self.assertEqual(len(failures), 1)
        self.assertIsNotNone(transport._process.poll())

    def test_seal_includes_late_eof_notice_and_previously_consumed_notices(self):
        transport = self.start("seal")
        self.assertEqual(transport.request("probe", {}, timeout=2), {"ok": True})
        self.assertEqual(transport.take_notifications()[0]["method"], "server/earlier")
        snapshot = transport.seal_notifications(timeout=2)
        self.assertEqual([item["method"] for item in snapshot], ["server/earlier", "model/rerouted"])
        self.assertTrue(transport._stdout_eof)
        self.assertFalse(transport._stdout_thread.is_alive())
        self.assertEqual(transport._process.returncode, 0)
        with self.assertRaises(RuntimeError):
            transport.seal_notifications(timeout=1)
        with self.assertRaises(RuntimeError):
            transport.request("probe", {}, timeout=1)
        transport.close()

    def test_seal_never_claims_complete_capture_after_timeout_partial_or_failed_exit(self):
        for mode in ("seal-hang", "seal-partial", "seal-error"):
            with self.subTest(mode=mode):
                transport = self.start(mode)
                transport.request("probe", {}, timeout=2)
                with self.assertRaises(AppServerProtocolError):
                    transport.seal_notifications(timeout=0.1)
                self.assertIsNotNone(transport._process.poll())
                with self.assertRaises(RuntimeError):
                    transport.seal_notifications(timeout=1)

    def test_diagnostics_retains_only_bounded_owned_stderr_tail(self):
        transport = self.start("diagnostic")
        transport._process.wait(timeout=2)
        transport._stdout_thread.join(timeout=2)
        transport._stderr_thread.join(timeout=2)
        metadata = transport.diagnostics()
        self.assertEqual(metadata["exit_code"], 7)
        self.assertTrue(metadata["stdout_eof"])
        self.assertEqual(len(metadata["stderr_tail"].encode("utf-8")), 16 * 1024)
        self.assertTrue(metadata["stderr_tail"].endswith("diagnostic-tail"))
        self.assertNotIn("prefix", metadata["stderr_tail"])
        transport.close()
        self.assertEqual(transport.diagnostics(), metadata)


if __name__ == "__main__":
    unittest.main()
