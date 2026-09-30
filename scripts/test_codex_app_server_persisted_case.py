"""Offline fixed persisted composition tests; no host or inference is launched."""

import io
import json
import tempfile
import unittest
from contextlib import redirect_stdout, redirect_stderr
from dataclasses import replace
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import patch

from scripts import codex_app_server_persisted_case as module
from scripts.codex_app_server_observer import AppServerReceipt, DispatchedConfiguration, ObservationRejected
from scripts.codex_route_catalogue import persisted_prompt
from scripts.codex_route_catalogue import development_catalogue, SelectionRejected
from scripts.codex_app_server_observer import AppServerObserver, ExecutionIntent
from scripts.codex_app_server_rpc import AppServerRpcError
from scripts.test_codex_app_server_observer import OfflineRpc, CWD, intent
from copy import deepcopy


def fixture(case, answer=None):
    events = [{"method": "thread/read", "response": {"thread": {"id": "offline-thread", "turns": [
        {"id": "offline-turn", "items": [{"type": "agentMessage", "text": case.marker_json if answer is None else answer}]}]}}}]
    return AppServerReceipt(case.intent.digest, case.intent.selection, None, None,
                            case.intent.prompt_digest, case.intent.invocation_key, "offline_fixture",
                            "completed_configured_route", "offline-thread", "offline-turn",
                            DispatchedConfiguration(module.MODEL, "openai", module.EFFORT),
                            "completed", None, json.dumps(events), False)


class PersistedCaseTests(unittest.TestCase):
    def setUp(self):
        self.case = module.prepare_persisted_case("/tmp/persisted-fixture")

    def test_fixed_key_prompt_binding_and_no_jev_stages(self):
        self.assertEqual(self.case.case_id, "s05-appserver-luna56-persisted-c39e8241")
        self.assertEqual(self.case.intent.invocation_key, "s05-owner-luna56-persisted-c39e8241")
        self.assertEqual(self.case.intent.prompt, persisted_prompt())
        self.assertEqual(self.case.intent.prompt_digest, self.case.intent.selection.task_input_digest)
        self.assertIs(self.case.intent.ephemeral_thread, False)
        for changed in ({"invocation_key": "other"}, {"prompt": "other"}, {"ephemeral_thread": True},
                        {"requested": self.case.intent.selection}, {"recommended": self.case.intent.selection}):
            with self.subTest(changed=changed), self.assertRaises(ObservationRejected):
                replace(self.case.intent, **changed)

    def test_preview_is_pure_and_cli_has_no_overrides(self):
        with patch.object(module, "execute_persisted_case") as execute, \
                patch.object(module, "build_one_off_launch_profile") as profile, \
                patch.object(module, "_private_directory") as directory, redirect_stdout(io.StringIO()) as output:
            self.assertEqual(module.main([]), 0)
        for effect in (execute, profile, directory):
            effect.assert_not_called()
        preview = json.loads(output.getvalue())
        self.assertEqual(preview["status"], "preview_only")
        self.assertEqual(preview["prompt_sha256"], self.case.intent.prompt_digest)
        for argument in ("--case-id", "--invocation-key", "--model", "--effort", "--evidence-dir", "--command"):
            with self.subTest(argument=argument), redirect_stdout(io.StringIO()), \
                    redirect_stderr(io.StringIO()), self.assertRaises(SystemExit):
                module.main([argument, "override"])

    def test_default_old_and_catalogue_bindings(self):
        from scripts.codex_app_server_one_off_case import prepare_one_off_case
        self.assertIs(intent().ephemeral_thread, True)
        self.assertIs(prepare_one_off_case(CWD).intent.ephemeral_thread, True)
        self.assertEqual(development_catalogue().digest,
                         "6bb52b041cda0ad8effbe18f71854d18a0a007b12f5d3f616e56ae3a887850be")
        for value in (0, 1, None, "false"):
            with self.subTest(value=value), self.assertRaises(ObservationRejected):
                replace(self.case.intent, ephemeral_thread=value)
        with self.assertRaises(ObservationRejected):
            replace(intent(), ephemeral_thread=False)
        with self.assertRaises(ObservationRejected):
            replace(prepare_one_off_case(CWD).intent, ephemeral_thread=False)
        with self.assertRaises(SelectionRejected):
            replace(self.case.intent.selection, catalogue_digest=development_catalogue().digest)
        with self.assertRaises(SelectionRejected):
            replace(self.case.intent.selection, task_input_digest="0" * 64)
        # The digest includes the strict persistence field (canonical expected payload).
        import hashlib
        from dataclasses import asdict
        payload = {"selection": asdict(self.case.intent.selection),
                   "prompt_sha256": self.case.intent.prompt_digest,
                   "invocation_key": self.case.intent.invocation_key, "cwd": self.case.intent.cwd,
                   "requested": None, "recommended": None, "ephemeral_thread": False}
        self.assertEqual(self.case.intent.digest, hashlib.sha256(json.dumps(payload,
                         sort_keys=True, separators=(",", ":")).encode()).hexdigest())

    def test_strict_marker_and_offline_never_live(self):
        receipt = fixture(self.case)
        self.assertEqual(module.evaluate_marker(self.case, receipt)["status"], "completed_offline_case")
        self.assertEqual(module.evaluate_marker(self.case, replace(receipt, notification_capture_complete=True))["status"],
                         "completed_offline_case")
        marker = json.loads(self.case.marker_json)
        for answer in (json.dumps(dict(marker, extra=True)), self.case.marker_json[:-1] + ',"kind":"duplicate"}',
                       "```json\n" + self.case.marker_json + "\n```", "Result: " + self.case.marker_json,
                       json.dumps(dict(marker, case_id="wrong")), "[]"):
            with self.subTest(answer=answer):
                self.assertEqual(module.evaluate_marker(self.case, fixture(self.case, answer))["status"],
                                 "configured_case_rejected")

    def test_owned_requires_exact_config_capture_and_scope(self):
        owned = replace(fixture(self.case), evidence_kind="owned_stdio", notification_capture_complete=True)
        self.assertEqual(module.evaluate_marker(self.case, owned)["status"], "completed_configured_case")
        for changed in ({"notification_capture_complete": False}, {"notification_capture_complete": 1},
                        {"invocation_key": "other"}, {"intent_digest": "other"},
                        {"evidence_kind": "imported"}, {"status": "unknown_after_reservation"},
                        {"dispatched_configured": DispatchedConfiguration("gpt-6-luna", "openai", "xhigh")}):
            with self.subTest(changed=changed):
                self.assertEqual(module.evaluate_marker(self.case, replace(owned, **changed))["status"],
                                 "configured_case_rejected")

    def test_private_composition_exclusive_intent_before_host_and_modes(self):
        calls = []
        profile = SimpleNamespace(argv=("offline",), digest="offline-profile", configured_mcp_count=0,
                                  configured_plugin_count=0)
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary) / "evidence"
            class Rpc:
                def __init__(self, argv, cwd):
                    self.cwd = cwd
                    self.assert_reserved = list(directory.glob("*-INTENT.json"))
                    calls.append(self)
                def __enter__(self): return self
                def __exit__(self, *args): pass
                def initialize(self, info): pass
            class Observer:
                def __init__(self, rpc, *, ledger_dir): pass
                def run_once(self, intent, *, completion_timeout):
                    if completion_timeout != 180: raise AssertionError("fixed bound")
                    return fixture(module.PreparedPersistedCase(module.CASE_ID, module.persisted_marker_json(), intent))
            kwargs = dict(evidence_dir=directory, profile_factory=lambda: profile,
                          rpc_factory=Rpc, observer_factory=Observer)
            result = module._execute_offline_case(**kwargs)
            self.assertEqual(result["status"], "completed_offline_case")
            self.assertEqual(len(calls[0].assert_reserved), 1)
            self.assertFalse(Path(calls[0].cwd).exists())
            self.assertFalse(calls[0].cwd.startswith("/Users/tony/Work/Projects/nuanu-ai-lab"))
            records = list(directory.glob("*.json"))
            self.assertEqual(len(records), 3)
            for record in records: self.assertEqual(record.stat().st_mode & 0o777, 0o600)
            self.assertEqual(directory.stat().st_mode & 0o777, 0o700)
            with self.assertRaises(FileExistsError): module._execute_offline_case(**kwargs)
            self.assertEqual(len(calls), 1)

    def test_launch_failure_burns_approval_without_receipt_or_sensitive_output(self):
        profile = SimpleNamespace(argv=("offline",), digest="offline", configured_mcp_count=0, configured_plugin_count=0)
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary) / "evidence"
            def fail(*args): raise RuntimeError("sensitive authentication detail")
            kwargs = dict(evidence_dir=directory, profile_factory=lambda: profile,
                          rpc_factory=fail, observer_factory=None)
            result = module._execute_offline_case(**kwargs)
            self.assertFalse(result["receipt_available"])
            self.assertNotIn("sensitive", json.dumps(result))
            self.assertEqual(len(list(directory.glob("*-INTENT.json"))), 1)
            self.assertEqual(len(list(directory.glob("*-RESULT.json"))), 1)
            self.assertEqual(len(list(directory.glob("*-RECEIPT.json"))), 0)
            with self.assertRaises(FileExistsError): module._execute_offline_case(**kwargs)


class PersistedObserverTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.ledger = Path(temporary.name) / "ledger"
        self.case = module.prepare_persisted_case(CWD)
        self.rpc = OfflineRpc()
        self.rpc.models["data"][0].update(model=module.MODEL,
            supportedReasoningEfforts=[{"reasoningEffort": module.EFFORT}])
        self.rpc.started.update(model=module.MODEL, reasoningEffort=module.EFFORT)
        self.rpc.started["thread"]["ephemeral"] = False
        items = self.rpc.read["thread"]["turns"][0]["items"]
        items[0].update(clientId=self.case.intent.invocation_key)
        items[0]["content"][0]["text"] = self.case.intent.prompt
        items[1]["text"] = self.case.marker_json
        self.observer = object.__new__(AppServerObserver)
        self.observer._configure(self.rpc, self.ledger, "offline_fixture")

    def test_persisted_readback_happy_and_one_use(self):
        receipt = self.observer.run_once(self.case.intent, completion_timeout=180)
        self.assertEqual(module.evaluate_marker(self.case, receipt)["status"], "completed_offline_case")
        starts = [params for method, params in self.rpc.calls if method == "thread/start"]
        self.assertIs(starts[0]["ephemeral"], False)
        reads = [params for method, params in self.rpc.calls if method == "thread/read"]
        self.assertEqual(reads, [{"threadId": "thread-1", "includeTurns": True}])
        self.assertEqual(self.rpc.seal_calls, [30])

    def test_persistence_confirmation_fails_before_turn(self):
        for value in (True, None, 0, "false"):
            with self.subTest(value=value):
                self.setUp()
                self.rpc.started["thread"]["ephemeral"] = value
                receipt = self.observer.run_once(self.case.intent, completion_timeout=180)
                self.assertEqual(receipt.status, "configured_route_rejected")
                self.assertNotIn("turn/start", [method for method, _ in self.rpc.calls])

    def test_read_rpc_error_safe_metadata_and_seal_once(self):
        self.observer._evidence_kind = "owned_stdio"  # explicitly private simulation, no live receipt claim
        request = self.rpc.request
        def failed(method, params, timeout=30):
            if method == "thread/read":
                error = AppServerRpcError("PRIVATE_SECRET_SENTINEL", -32600)
                error.args = ("PRIVATE_SECRET_SENTINEL",)
                raise error
            return request(method, params, timeout)
        self.rpc.request = failed
        receipt = self.observer.run_once(self.case.intent, completion_timeout=180)
        errors = [event["error"] for event in json.loads(receipt.events_json) if "error" in event]
        self.assertEqual(errors, [{"error_type": "AppServerRpcError", "method": "thread/read", "code": -32600}])
        self.assertEqual(receipt.status, "unknown_after_reservation")
        self.assertTrue(receipt.notification_capture_complete)
        self.assertEqual(self.rpc.seal_calls, [30])
        self.assertEqual(module.evaluate_marker(self.case, receipt)["status"], "configured_case_rejected")
        for record in self.ledger.glob("*.json"):
            self.assertNotIn("PRIVATE_SECRET_SENTINEL", record.read_text())

    def test_semantic_failure_seals_but_rejects_and_failed_seal_is_incomplete(self):
        for failed_seal in (False, True):
            with self.subTest(failed_seal=failed_seal):
                self.setUp()
                self.observer._evidence_kind = "owned_stdio"
                self.rpc.read["thread"]["id"] = "wrong"
                if failed_seal:
                    self.rpc.seal_error = RuntimeError("PRIVATE_SECRET_SENTINEL")
                receipt = self.observer.run_once(self.case.intent, completion_timeout=180)
                self.assertEqual(receipt.status, "configured_route_rejected")
                self.assertEqual(receipt.notification_capture_complete, not failed_seal)
                self.assertEqual(self.rpc.seal_calls, [30])
                self.assertEqual(module.evaluate_marker(self.case, receipt)["status"], "configured_case_rejected")
                self.assertNotIn("PRIVATE_SECRET_SENTINEL", receipt.events_json)


if __name__ == "__main__":
    unittest.main()
