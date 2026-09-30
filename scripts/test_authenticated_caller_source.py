"""Synthetic owned Unix peers only; no daemon, DB or provider authorization proof."""
from dataclasses import replace
import hashlib
import io
import json
import os
from pathlib import Path
import socket
import tempfile
import threading
from types import SimpleNamespace
import unittest
from unittest.mock import patch

from scripts.authenticated_caller_source import (AuthenticatedCurrentSource,
    _host_adapter_context, _auth, _decode, _socket_identity)
from scripts.bounded_caller_route_launcher import (_execute_owned, _executor_route,
    main, INSTALLED_EXECUTABLE)
from scripts.caller_host_routing import CallerRoutingRejected, _json
from scripts.test_caller_host_routing import golden, request, PROMPT, ScriptedRpc
from scripts.codex_app_server_observer import AppServerObserver
from scripts.test_codex_app_server_observer import CWD


class SyntheticUnixSourceTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name).resolve()
        os.chmod(self.root, 0o700)
        self.config = self.root / "host.json"
        self.config.write_text(_json({"host_id": "11111111-1111-1111-1111-111111111111", "credential": "a" * 64}))
        os.chmod(self.config, 0o600)
        self.path = self.root / "source.sock"
        self.listener = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        self.listener.bind(str(self.path))
        os.chmod(self.path, 0o600)
        self.listener.listen(1)
        self.listener.settimeout(3)
        m = golden()
        self.context = _host_adapter_context(socket_path=self.path, config_path=self.config,
            workspace_key="synthetic", native_session_id="22222222-2222-2222-2222-222222222222",
            workspace_id=m["workspace_id"], actor_id=m["invoking_actor_id"], session_id=m["invoking_session_id"])
        self.digest = hashlib.sha256(PROMPT.encode()).hexdigest()
        self.ledger = self.root / "ledger"
        self.ledger.mkdir(mode=0o700)
        m["input_sha256"] = self.digest
        m["invocation_key"] = request().invocation_key
        self.material = m
        self.requests = []

    def tearDown(self):
        self.listener.close()
        self.temp.cleanup()

    def reply(self, transform=None, raw=None, trailing=False, hold=False, delay=0):
        m = self.material
        material_json = _json(m)
        result = {"material": m, "material_json": material_json,
            "material_sha256": hashlib.sha256(material_json.encode()).hexdigest(),
            "authorization_scope": "current_authenticated_read_only_snapshot", "actions": [],
            "recommended_action": None}
        response = {"status": "ok", "result": result}
        if transform:
            transform(response)
        def peer():
            with self.listener.accept()[0] as stream:
                data = bytearray()
                while b"\n" not in data:
                    part = stream.recv(65536)
                    if not part:
                        return
                    data.extend(part)
                self.requests.append(json.loads(data))
                if delay:
                    threading.Event().wait(delay)
                stream.sendall((raw if raw is not None else _json(response).encode()) + b"\n")
                if trailing or hold:
                    threading.Event().wait(0.1)
                if trailing:
                    stream.sendall(b'{"status":"error","error":"synthetic"}\n')
        thread = threading.Thread(target=peer)
        thread.start()
        return thread

    def resolve(self, **kw):
        return AuthenticatedCurrentSource(self.context, **kw).resolve_current(request(), input_sha256=self.digest)

    def test_direct_fresh_wire_and_full_binding(self):
        thread = self.reply()
        snapshot = self.resolve()
        thread.join(3)
        self.assertEqual(snapshot.material_json, _json(self.material))
        wire = self.requests[0]
        self.assertEqual(wire["api_version"], 2)
        self.assertEqual(wire["tool_name"], "prepare_model_route_host_selection")
        self.assertEqual(wire["arguments"]["input_sha256"], self.digest)
        self.assertEqual(wire["context"]["native_session_id"], self.context.native_session_id)
        self.assertEqual(set(wire["arguments"]), set(request().__dict__) | {"input_sha256"})

    def test_source_denials_and_projection_tampering(self):
        mutations = [lambda r: r.update(status="error"),
            lambda r: r["result"].update(authorization_scope="imported"),
            lambda r: r["result"].update(material_sha256="0" * 64),
            lambda r: r["result"]["material"].update(invocation_key="foreign")]
        for mutate in mutations:
            thread = self.reply(mutate)
            with self.assertRaises(CallerRoutingRejected):
                self.resolve()
            thread.join(3)

    def test_capacity_denial(self):
        thread = self.reply()
        with self.assertRaises(CallerRoutingRejected):
            self.resolve(output_capacity=1)
        thread.join(3)
        for value in (0, True, 8 * 1024 * 1024 + 1):
            with self.assertRaises(CallerRoutingRejected):
                AuthenticatedCurrentSource(self.context, output_capacity=value)

    def test_untrusted_identity_pin_denies_foreign_result(self):
        source = AuthenticatedCurrentSource(replace(self.context, actor_id="33333333-3333-3333-3333-333333333333"))
        thread = self.reply()
        with self.assertRaises(CallerRoutingRejected):
            source.resolve_current(request(), input_sha256=self.digest)
        thread.join(3)

    def test_credential_shape_permission_symlink_and_size(self):
        os.chmod(self.config, 0o640)
        with self.assertRaises(CallerRoutingRejected):
            _auth(self.config)
        os.chmod(self.config, 0o600)
        link = self.root / "link"
        link.symlink_to(self.config)
        with self.assertRaises(CallerRoutingRejected):
            _auth(link)
        self.config.write_bytes(b"x" * 4097)
        with self.assertRaises(CallerRoutingRejected):
            _auth(self.config)

    def test_socket_permissions_and_substitution(self):
        os.chmod(self.path, 0o666)
        with self.assertRaises(CallerRoutingRejected):
            _socket_identity(self.path)
        os.chmod(self.path, 0o600)
        thread = self.reply()
        actual = _socket_identity(self.path)
        with patch("scripts.authenticated_caller_source._socket_identity", side_effect=[actual, ((0, 0, 0), (0, 0, 0))]):
            with self.assertRaises(CallerRoutingRejected):
                self.resolve()
        # Connected peer receives EOF before any credential frame.
        thread.join(3)

    def test_closed_decoder(self):
        for raw in (b'{"a":1,"a":2}', b'{"x":NaN}', b'{"x":1.1}', b'\xff'):
            with self.assertRaises(CallerRoutingRejected):
                _decode(raw)

    def test_allowlist_exact_pairs(self):
        for model, effort in (("gpt-6-luna", "xhigh"), ("gpt-6.1-sol", "medium")):
            _executor_route({"selected_route": {"provider": "openai", "model": model, "effort": effort}})
        for provider, model, effort in (("openai", "gpt-5.6-luna", "xhigh"), ("openai", "gpt-6-luna", "high"), ("other", "gpt-6.1-sol", "medium"), ("openai", "gpt-6.1", "medium")):
            with self.assertRaises(CallerRoutingRejected):
                _executor_route({"selected_route": {"provider": provider, "model": model, "effort": effort}})

    def test_launcher_denies_unbounded_prompt_before_source_or_child(self):
        with patch("scripts.bounded_caller_route_launcher.OwnedAppServerRpc") as child:
            with self.assertRaises(CallerRoutingRejected):
                _execute_owned(host_context=self.context, request=request(), prompt="x" * 16385,
                               cwd=str(self.root), ledger_dir=self.ledger, executable=INSTALLED_EXECUTABLE)
            child.assert_not_called()

    def test_synthetic_launcher_refreshes_and_keeps_unknown_actual(self):
        class SyntheticChild(ScriptedRpc):
            def __enter__(self):
                return self
            def __exit__(self, *args):
                pass
            def initialize(self, identity):
                pass
        rpc = SyntheticChild()
        def observer_factory(rpc, *, ledger_dir):
            observer = object.__new__(AppServerObserver)
            observer._configure(rpc, ledger_dir, "offline_fixture")
            return observer
        threads = [self.reply(), self.reply()]
        with patch("scripts.bounded_caller_route_launcher._host_profile", return_value=SimpleNamespace(argv=("synthetic-no-process",), digest="a" * 64)) as profile, \
                patch("scripts.bounded_caller_route_launcher.OwnedAppServerRpc", return_value=rpc), \
                patch("scripts.bounded_caller_route_launcher.AppServerObserver", side_effect=observer_factory):
            receipt = _execute_owned(host_context=self.context, request=request(), prompt=PROMPT,
                cwd=CWD, ledger_dir=self.ledger, executable=INSTALLED_EXECUTABLE)
        for thread in threads:
            thread.join(3)
        self.assertEqual(len(self.requests), 2)
        self.assertEqual(receipt.evidence_kind, "offline_fixture")
        self.assertEqual(receipt.status, "completed_configured_route", receipt.failure)
        self.assertIsNone(receipt.observed_actual)
        self.assertEqual(receipt.source_binding_json, _json(self.material))
        profile.assert_called_once_with("gpt-6.1-sol", "medium", INSTALLED_EXECUTABLE)

    def test_delayed_extra_frame_and_missing_eof_deny(self):
        thread = self.reply(trailing=True)
        with self.assertRaises(CallerRoutingRejected):
            self.resolve()
        thread.join(3)
        thread = self.reply(hold=True)
        with patch("scripts.authenticated_caller_source._SOURCE_CALL_TIMEOUT", 0.03):
            with self.assertRaises(CallerRoutingRejected):
                self.resolve()
        thread.join(3)

    def test_response_wait_uses_global_budget_beyond_short_io_stage_cap(self):
        thread = self.reply(delay=0.08)
        with patch("scripts.authenticated_caller_source._SOURCE_CALL_TIMEOUT", 0.5), \
                patch("scripts.authenticated_caller_source._SOURCE_IO_TIMEOUT", 0.01):
            snapshot = self.resolve()
        thread.join(3)
        self.assertEqual(snapshot.material_json, _json(self.material))

    def cli_pins(self):
        return {**request().__dict__, "expected_workspace_id": self.context.workspace_id,
            "expected_actor_id": self.context.actor_id, "expected_session_id": self.context.session_id}

    def cli_environment(self):
        return {"TECT_HOST_CONFIG": str(self.config), "TECT_SOCKET": str(self.path),
            "TECT_WORKSPACE_KEY": "synthetic", "TECT_NATIVE_SESSION_ID": self.context.native_session_id,
            "TECT_CALLER_LEDGER_DIR": str(self.ledger), "TECT_CALLER_CODEX_EXECUTABLE": INSTALLED_EXECUTABLE}

    def test_cli_missing_context_and_malformed_pins_deny_before_source_or_child(self):
        with patch.dict(os.environ, {}, clear=True), \
                patch("scripts.bounded_caller_route_launcher._execute_owned") as execute, \
                patch("sys.stdout", io.StringIO()):
            self.assertEqual(main(["--execute-owned", "--pins-json", _json(self.cli_pins())]), 1)
            for data in ('{"a":1,"a":2}', _json({**self.cli_pins(), "authorized": True}), _json({**self.cli_pins(), "expected_task_revision": True})):
                self.assertEqual(main(["--execute-owned", "--pins-json", data]), 1)
            execute.assert_not_called()

    def test_historical_model_route_denied_before_profile_or_observer(self):
        row = next(row for row in self.material["preparation"]["catalogue"]["routes"] if row["id"] == "owner-sol61")
        row["model"] = "gpt-5.6-sol"
        recorded = next(row for row in self.material["decision"]["prepared"]["catalogue"]["routes"] if row["id"] == "owner-sol61")
        recorded["model"] = "gpt-5.6-sol"
        selected = {"route_id": row["id"], **{key: row[key] for key in ("provider", "model", "effort")}}
        self.material["selected_route"] = selected
        self.material["configured_route"] = selected
        thread = self.reply()
        with patch("scripts.bounded_caller_route_launcher._host_profile") as profile, \
                patch("scripts.bounded_caller_route_launcher.AppServerObserver") as observer:
            with self.assertRaises(CallerRoutingRejected):
                _execute_owned(host_context=self.context, request=request(), prompt=PROMPT,
                    cwd=CWD, ledger_dir=self.ledger, executable=INSTALLED_EXECUTABLE)
            profile.assert_not_called()
            observer.assert_not_called()
        thread.join(3)

    def test_synthetic_cli_once_and_reopened_ledger_blocks_second_child(self):
        class SyntheticChild(ScriptedRpc):
            def __enter__(self):
                return self
            def __exit__(self, *args):
                pass
            def initialize(self, identity):
                pass
        rpc = SyntheticChild()
        def observer_factory(rpc, *, ledger_dir):
            observer = object.__new__(AppServerObserver)
            observer._configure(rpc, ledger_dir, "offline_fixture")
            return observer
        prompt_path = self.root / "prompt"
        prompt_path.write_bytes(PROMPT.encode())
        args = ["--execute-owned", "--pins-json", _json(self.cli_pins()), "--prompt-file", str(prompt_path), "--cwd", CWD]
        threads = [self.reply(), self.reply()]
        with patch.dict(os.environ, self.cli_environment(), clear=True), \
                patch("scripts.bounded_caller_route_launcher._host_profile", return_value=SimpleNamespace(argv=("synthetic",), digest="a" * 64)), \
                patch("scripts.bounded_caller_route_launcher.OwnedAppServerRpc", return_value=rpc) as child, \
                patch("scripts.bounded_caller_route_launcher.AppServerObserver", side_effect=observer_factory), \
                patch("sys.stdout", io.StringIO()) as output:
            self.assertEqual(main(args), 0)
            self.assertEqual(main(args), 1)
            self.assertEqual(child.call_count, 1)
            results = [json.loads(line) for line in output.getvalue().splitlines()]
            self.assertIsNone(results[0]["observed_actual"])
            self.assertEqual(results[0]["evidence_kind"], "offline_fixture")
        for thread in threads:
            thread.join(3)
        self.assertEqual(len(self.requests), 2)

    def test_historical_observer_reservations_deny_before_source_or_child(self):
        for kind in ("malformed", "unreadable", "symlink", "directory"):
            with self.subTest(kind=kind):
                ledger = self.root / ("historical-" + kind)
                ledger.mkdir(mode=0o700)
                entry = ledger / (hashlib.sha256(request().invocation_key.encode()).hexdigest() + ".00-reserved.json")
                if kind == "symlink":
                    entry.symlink_to(self.root / "missing-target")
                elif kind == "directory":
                    entry.mkdir()
                else:
                    entry.write_bytes(b"not json")
                    entry.chmod(0 if kind == "unreadable" else 0o600)
                with patch("scripts.bounded_caller_route_launcher._ExecutorSource") as source, \
                        patch("scripts.bounded_caller_route_launcher.OwnedAppServerRpc") as child:
                    with self.assertRaises(CallerRoutingRejected):
                        _execute_owned(host_context=self.context, request=request(), prompt=PROMPT,
                            cwd=CWD, ledger_dir=ledger, executable=INSTALLED_EXECUTABLE)
                    source.assert_not_called()
                    child.assert_not_called()


if __name__ == "__main__":
    unittest.main()
