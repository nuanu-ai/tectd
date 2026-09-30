"""Offline controlled-case tests; patched composition starts no host or model."""

import io
import json
import tempfile
import unittest
from contextlib import redirect_stdout
from dataclasses import replace
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import patch

from scripts import codex_app_server_case as case_module
from scripts.codex_app_server_observer import AppServerReceipt, DispatchedConfiguration


CASE_ID = "s05-appserver-luna-20260930-01"
KEY = "s05-appserver-luna-one-use-20260930-01"


def fixture_receipt(case, answer=None):
    answer = case.marker_json if answer is None else answer
    events = [{"method": "thread/read", "response": {"thread": {"id": "thread-fixture", "turns": [
        {"id": "turn-fixture", "items": [{"type": "agentMessage", "text": answer}]}]}}}]
    return AppServerReceipt(case.intent.digest, case.intent.selection, None, None,
                            case.intent.prompt_digest, case.intent.invocation_key, "offline_fixture",
                            "completed_configured_route", "thread-fixture", "turn-fixture",
                            DispatchedConfiguration("gpt-6-luna", "openai", "xhigh"),
                            "completed", None, json.dumps(events), False)


class CaseTests(unittest.TestCase):
    def setUp(self):
        self.case = case_module.prepare_case(CASE_ID, KEY, "/tmp/s05-offline-case")

    def test_fixed_route_prompt_hash_and_absent_jev_stages(self):
        self.assertEqual(self.case.intent.selection.route_id, case_module.SELECTED_ROUTE)
        self.assertEqual((self.case.intent.selection.model, self.case.intent.selection.effort),
                         ("gpt-6-luna", "xhigh"))
        self.assertEqual(self.case.intent.selection.purpose, "routine")
        self.assertEqual(self.case.intent.prompt, case_module.fixed_prompt(CASE_ID))
        self.assertEqual(self.case.intent.prompt_digest, self.case.intent.selection.task_input_digest)
        self.assertIsNone(self.case.intent.requested)
        self.assertIsNone(self.case.intent.recommended)
        self.assertNotEqual(case_module.fixed_prompt("other-case"), self.case.intent.prompt)
        self.assertNotIn("actual_model", json.loads(self.case.marker_json))

    def test_exact_marker_validates_only_as_offline_fixture(self):
        result = case_module.evaluate_marker(self.case, fixture_receipt(self.case))
        self.assertEqual(result["status"], "completed_offline_case")
        self.assertTrue(result["marker_matches"])
        self.assertFalse(result["notification_capture_complete"])
        self.assertIsNone(result["observed_actual"])

    def test_owned_case_requires_exact_true_complete_notification_capture(self):
        # Synthetic owned-labelled values exercise logic, not live host proof.
        owned = replace(fixture_receipt(self.case), evidence_kind="owned_stdio")
        for missing_or_false in (False, None, 1, "true", "missing"):
            with self.subTest(capture=missing_or_false):
                changed = replace(owned, notification_capture_complete=missing_or_false)
                if missing_or_false == "missing":
                    object.__delattr__(changed, "notification_capture_complete")
                result = case_module.evaluate_marker(self.case, changed)
                self.assertEqual(result["status"], "configured_case_rejected")
                self.assertFalse(result["marker_matches"])
                self.assertFalse(result["notification_capture_complete"])
        complete = replace(owned, notification_capture_complete=True)
        self.assertEqual(case_module.evaluate_marker(self.case, complete)["status"],
                         "completed_configured_case")

    def test_offline_seam_never_emits_production_completion_even_with_capture_flag(self):
        offline = replace(fixture_receipt(self.case), notification_capture_complete=True)
        result = case_module.evaluate_marker(self.case, offline)
        self.assertEqual(result["status"], "completed_offline_case")
        self.assertNotEqual(result["status"], "completed_configured_case")

    def test_marker_extra_duplicate_wrong_case_or_prose_reject(self):
        marker = json.loads(self.case.marker_json)
        wrong = dict(marker, case_id="other-case")
        answers = [json.dumps(wrong), json.dumps(dict(marker, model="I am Luna")),
                   self.case.marker_json[:-1] + ',"case_id":"duplicate"}',
                   "```json\n" + self.case.marker_json + "\n```", "Result: " + self.case.marker_json]
        for answer in answers:
            with self.subTest(answer=answer):
                result = case_module.evaluate_marker(self.case, fixture_receipt(self.case, answer))
                self.assertFalse(result["marker_matches"])
                self.assertEqual(result["status"], "configured_case_rejected")

    def test_receipt_context_and_observer_failure_cannot_pass_marker(self):
        receipt = fixture_receipt(self.case)
        for changes in ({"intent_digest": "forged"}, {"prompt_digest": "forged"},
                        {"status": "unknown_after_reservation"}, {"invocation_key": "other-key"},
                        {"dispatched_configured": DispatchedConfiguration("gpt-6.1-sol", "openai", "medium")}):
            with self.subTest(changes=changes):
                result = case_module.evaluate_marker(self.case, replace(receipt, **changes))
                self.assertFalse(result["marker_matches"])
        with self.assertRaises(case_module.CaseRejected):
            case_module.evaluate_marker(self.case, {"accepted": True})

    def test_preview_without_switch_has_no_launch_or_writes(self):
        with tempfile.TemporaryDirectory() as temporary:
            evidence = Path(temporary) / "not-created"
            with patch.object(case_module, "execute_case") as execute, redirect_stdout(io.StringIO()) as output:
                result = case_module.main(["--case-id", CASE_ID, "--invocation-key", KEY,
                                           "--evidence-dir", str(evidence)])
            self.assertEqual(result, 0)
            execute.assert_not_called()
            self.assertFalse(evidence.exists())
            self.assertEqual(json.loads(output.getvalue())["status"], "preview_only")

    def test_fixed_offline_composition_evidence_and_duplicate_prelaunch_fence(self):
        calls = []

        class OfflineRpc:
            def __init__(self, argv, cwd):
                calls.append(("constructed", argv, cwd))

            def __enter__(self):
                return self

            def __exit__(self, *args):
                calls.append(("closed",))

            def initialize(self, client_info):
                calls.append(("initialize", client_info))

        class OfflineObserver:
            def __init__(self, rpc, *, ledger_dir):
                calls.append(("observer", ledger_dir))

            def run_once(self, intent, *, completion_timeout):
                calls.append(("run_once", intent, completion_timeout))
                prepared = case_module.PreparedCase(CASE_ID, case_module.marker_json(CASE_ID), intent)
                return fixture_receipt(prepared)

        profile = SimpleNamespace(argv=("fixed-test-profile",), digest="offline-profile-digest",
                                  configured_mcp_count=2, configured_plugin_count=3)
        with tempfile.TemporaryDirectory() as temporary:
            evidence = Path(temporary) / "evidence"
            with patch.object(case_module, "build_launch_profile", return_value=profile) as build, \
                    patch.object(case_module, "OwnedAppServerRpc", OfflineRpc), \
                    patch.object(case_module, "AppServerObserver", OfflineObserver):
                result = case_module.execute_case(case_id=CASE_ID, invocation_key=KEY, evidence_dir=evidence)
                self.assertEqual(result["status"], "completed_offline_case")
                build.assert_called_once_with("gpt-6-luna", "xhigh")
                cwd = calls[0][2]
                self.assertFalse(Path(cwd).exists())
                self.assertEqual(calls[0][1], profile.argv)
                self.assertEqual(len(list(evidence.glob("APP_SERVER-*.json"))), 3)
                for path in evidence.glob("APP_SERVER-*.json"):
                    self.assertEqual(path.stat().st_mode & 0o777, 0o600)
                before = len(calls)
                with self.assertRaises(FileExistsError):
                    case_module.execute_case(case_id=CASE_ID, invocation_key=KEY, evidence_dir=evidence)
                self.assertEqual(len(calls), before)

    def test_invalid_ids_or_public_directory_reject_before_launch(self):
        with self.assertRaises(case_module.CaseRejected):
            case_module.prepare_case("../bad-id", KEY, "/tmp/s05")
        with self.assertRaises(case_module.CaseRejected):
            case_module.prepare_case(CASE_ID, "bad key", "/tmp/s05")
        with tempfile.TemporaryDirectory() as temporary:
            evidence = Path(temporary) / "public"
            evidence.mkdir(mode=0o755)
            with patch.object(case_module, "OwnedAppServerRpc") as rpc, self.assertRaises(case_module.CaseRejected):
                case_module.execute_case(case_id=CASE_ID, invocation_key=KEY, evidence_dir=evidence)
            rpc.assert_not_called()

    def test_launch_failure_retained_without_fabricated_receipt_or_retry(self):
        profile = SimpleNamespace(argv=("fixed-offline-profile",), digest="offline",
                                  configured_mcp_count=0, configured_plugin_count=0)
        with tempfile.TemporaryDirectory() as temporary:
            evidence = Path(temporary) / "evidence"
            with patch.object(case_module, "build_launch_profile", return_value=profile), \
                    patch.object(case_module, "OwnedAppServerRpc", side_effect=RuntimeError("sensitive contents")) as rpc:
                result = case_module.execute_case(case_id=CASE_ID, invocation_key=KEY, evidence_dir=evidence)
            rpc.assert_called_once()
            self.assertEqual(result["status"], "case_failed_without_retry")
            self.assertFalse(result["receipt_available"])
            self.assertNotIn("sensitive", json.dumps(result))
            self.assertTrue((evidence / f"APP_SERVER-{CASE_ID}-RESULT.json").exists())
            self.assertFalse((evidence / f"APP_SERVER-{CASE_ID}-RECEIPT.json").exists())


if __name__ == "__main__":
    unittest.main()
