"""Offline fixed one-off composition tests; no host or inference is launched."""

import io
import json
import tempfile
import unittest
from contextlib import redirect_stdout, redirect_stderr
from dataclasses import replace
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import patch

from scripts import codex_app_server_one_off_case as module
from scripts.codex_app_server_observer import AppServerReceipt, DispatchedConfiguration, ObservationRejected
from scripts.codex_route_catalogue import one_off_prompt


def fixture(case, answer=None):
    events = [{"method": "thread/read", "response": {"thread": {"id": "offline-thread", "turns": [
        {"id": "offline-turn", "items": [{"type": "agentMessage", "text": case.marker_json if answer is None else answer}]}]}}}]
    return AppServerReceipt(case.intent.digest, case.intent.selection, None, None,
                            case.intent.prompt_digest, case.intent.invocation_key, "offline_fixture",
                            "completed_configured_route", "offline-thread", "offline-turn",
                            DispatchedConfiguration(module.MODEL, "openai", module.EFFORT),
                            "completed", None, json.dumps(events), False)


class OneOffCaseTests(unittest.TestCase):
    def setUp(self):
        self.case = module.prepare_one_off_case("/tmp/one-off-fixture")

    def test_fixed_key_prompt_binding_and_no_jev_stages(self):
        self.assertEqual(self.case.case_id, "s05-appserver-luna56-oneoff-7f29a6f6")
        self.assertEqual(self.case.intent.invocation_key, "s05-owner-luna56-oneoff-7f29a6f6")
        self.assertEqual(self.case.intent.prompt, one_off_prompt())
        self.assertEqual(self.case.intent.prompt_digest, self.case.intent.selection.task_input_digest)
        for changed in ({"invocation_key": "other"}, {"prompt": "other"},
                        {"requested": self.case.intent.selection}, {"recommended": self.case.intent.selection}):
            with self.subTest(changed=changed), self.assertRaises(ObservationRejected):
                replace(self.case.intent, **changed)

    def test_preview_is_pure_and_cli_has_no_overrides(self):
        with patch.object(module, "execute_one_off_case") as execute, \
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
                    return fixture(module.PreparedOneOffCase(module.CASE_ID, module.one_off_marker_json(), intent))
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


if __name__ == "__main__":
    unittest.main()
