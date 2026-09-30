"""Offline test composition only: fake records are not authenticated evidence."""

import json
import tempfile
import unittest
from copy import deepcopy
from dataclasses import FrozenInstanceError, replace
from pathlib import Path

from scripts.codex_app_server_observer import (
    AppServerObserver, ExecutionIntent, ObservationRejected, _sha,
)
from scripts.codex_route_catalogue import development_catalogue, select_route


PROMPT = "Return the exact text: bounded S05 case."
CWD = "/tmp/owned-s05-fixture"


def intent(key="fresh-s05-key"):
    catalogue = development_catalogue()
    route = catalogue.routes[0]
    selection = select_route(route_id=route.route_id, model=route.model, effort=route.effort,
                             purpose=route.purpose, task_input_digest=_sha(PROMPT),
                             catalogue_version=catalogue.version, catalogue_digest=catalogue.digest)
    return ExecutionIntent(selection, PROMPT, key, CWD)


def readback():
    return {"thread": {"id": "thread-1", "cwd": CWD, "modelProvider": "openai", "turns": [
        {"id": "turn-1", "status": "completed", "itemsView": "full", "error": None,
         "items": [{"id": "user-1", "type": "userMessage", "clientId": "fresh-s05-key",
                    "content": [{"type": "text", "text": PROMPT, "text_elements": []}]},
                   {"id": "answer-1", "type": "agentMessage", "text": "bounded S05 case."}]}]}}


class OfflineRpc:
    def __init__(self):
        self.calls = []
        self.models = {"data": [{"model": "gpt-6.1-sol", "supportedReasoningEfforts": [
            {"reasoningEffort": "medium", "description": "fixture effort"}]}], "nextCursor": None}
        self.mcp = {"data": [], "nextCursor": None}
        self.started = {"model": "gpt-6.1-sol", "modelProvider": "openai", "reasoningEffort": "medium",
                        "approvalPolicy": "never", "cwd": CWD, "sandbox": {"type": "readOnly"},
                        "thread": {"id": "thread-1"}}
        self.turn_started = {"turn": {"id": "turn-1", "status": "inProgress", "items": []}}
        self.completed = {"method": "turn/completed", "params": {"threadId": "thread-1", "turn": {
            "id": "turn-1", "status": "completed", "items": []}}}
        self.read = readback()
        self.notifications = []
        self.during_seal = []
        self.extra_sealed_notifications = []
        self.all_notifications = []
        self.seal_error = None
        self.seal_calls = []
        self.fail_method = None

    def request(self, method, params, timeout=30):
        self.calls.append((method, deepcopy(params)))
        if self.fail_method == method:
            raise TimeoutError("unknown response")
        return deepcopy({"model/list": self.models, "mcpServerStatus/list": self.mcp,
                         "thread/start": self.started, "turn/start": self.turn_started,
                         "thread/read": self.read}[method])

    def wait_notification(self, method, predicate, timeout):
        self.calls.append((method, {}))
        # Check actual transport contract: predicates receive notification envelope.
        if not predicate(deepcopy(self.completed)):
            raise TimeoutError("no matching host notification")
        self.all_notifications.append(deepcopy(self.completed))
        return deepcopy(self.completed)

    def take_notifications(self, method=None):
        result, self.notifications = self.notifications, []
        return deepcopy(result)

    def seal_notifications(self, timeout=30):
        self.seal_calls.append(timeout)
        if len(self.seal_calls) > 1:
            raise RuntimeError("offline fixture capture sealed twice")
        if self.seal_error is not None:
            raise self.seal_error
        snapshot = self.all_notifications + self.notifications + self.during_seal + self.extra_sealed_notifications
        self.notifications = []
        self.during_seal = []
        return deepcopy(snapshot)


class ObserverTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.ledger = Path(temporary.name) / "ledger"
        self.rpc = OfflineRpc()
        self.case = intent()
        self.observer = self.offline_observer()

    def offline_observer(self):
        # Deliberate PRIVATE test seam; no public arbitrary transport acceptance.
        observer = object.__new__(AppServerObserver)
        observer._configure(self.rpc, self.ledger, "offline_fixture")
        return observer

    def test_exact_owned_public_transport_required(self):
        for forged in (self.rpc, True, {"owned": True}, "host accepted"):
            with self.subTest(forged=forged), self.assertRaises(ObservationRejected):
                AppServerObserver(forged, ledger_dir=self.ledger)

    def test_exact_prompt_selection_ids_config_and_actual_unknown(self):
        receipt = self.observer.run_once(self.case)
        self.assertEqual(receipt.status, "completed_configured_route")
        self.assertEqual(receipt.host_kind, "APP_SERVER")
        self.assertEqual(receipt.evidence_kind, "offline_fixture")
        self.assertFalse(receipt.notification_capture_complete)
        self.assertEqual((receipt.thread_id, receipt.turn_id), ("thread-1", "turn-1"))
        self.assertEqual(receipt.dispatched_configured.model, "gpt-6.1-sol")
        self.assertEqual(receipt.dispatched_configured.effort, "medium")
        self.assertIsNone(receipt.observed_actual)
        self.assertIsNone(receipt.requested)
        self.assertIsNone(receipt.recommended)
        calls = dict(self.rpc.calls)
        self.assertEqual(calls["thread/start"]["config"], {"model_reasoning_effort": "medium"})
        self.assertIs(calls["thread/start"]["allowProviderModelFallback"], False)
        self.assertEqual(calls["thread/start"]["sandbox"], "read-only")
        self.assertEqual(calls["turn/start"]["input"][0]["text"], PROMPT)
        self.assertEqual(calls["turn/start"]["effort"], "medium")
        self.assertEqual(receipt.intent_digest, self.case.digest)
        self.assertEqual(receipt.prompt_digest, _sha(PROMPT))
        self.assertEqual(self.rpc.seal_calls, [30])
        captured_completions = [
            event["notification"] for event in json.loads(receipt.events_json)
            if event.get("notification", {}).get("method") == "turn/completed"
        ]
        self.assertEqual(captured_completions, [self.rpc.completed])
        self.assertEqual(len(list(self.ledger.glob("*.json"))), 3)
        with self.assertRaises(FrozenInstanceError):
            receipt.status = "forged"

    def test_prompt_and_intent_binding_reject_mismatch(self):
        with self.assertRaisesRegex(ObservationRejected, "exact prompt"):
            replace(self.case, prompt="Changed task")
        self.assertNotEqual(replace(self.case, invocation_key="other-key").digest, self.case.digest)
        with self.assertRaises(ObservationRejected):
            self.observer.run_once({"accepted": True})
        self.assertEqual(self.rpc.calls, [])

    def test_start_config_mismatch_or_missing_never_starts_turn(self):
        for field, value in (("model", "gpt-6-luna"), ("modelProvider", "other"),
                             ("reasoningEffort", "xhigh"), ("reasoningEffort", None),
                             ("sandbox", {"type": "dangerFullAccess"})):
            with self.subTest(field=field):
                self.setUp()
                self.rpc.started[field] = value
                receipt = self.observer.run_once(self.case)
                self.assertEqual(receipt.status, "configured_route_rejected")
                self.assertNotIn("turn/start", [method for method, _ in self.rpc.calls])

    def test_missing_model_effort_or_nonempty_mcp_never_starts_thread(self):
        for changed in ("models", "effort", "missing_mcp", "mcp"):
            with self.subTest(changed=changed):
                self.setUp()
                if changed == "models":
                    self.rpc.models = {"data": []}
                elif changed == "effort":
                    self.rpc.models["data"][0]["supportedReasoningEfforts"] = []
                elif changed == "missing_mcp":
                    self.rpc.mcp = {"empty": True}
                else:
                    self.rpc.mcp = {"data": [{"name": "forbidden"}]}
                self.assertEqual(self.observer.run_once(self.case).status, "configured_route_rejected")
                self.assertNotIn("thread/start", [method for method, _ in self.rpc.calls])

    def test_readback_missing_wrong_prompt_ids_tool_or_outcome_reject(self):
        for changed in ("prompt", "thread_id", "turn_id", "status", "tool", "missing_user", "items_view"):
            with self.subTest(changed=changed):
                self.setUp()
                thread = self.rpc.read["thread"]
                turn = thread["turns"][0]
                if changed == "prompt":
                    turn["items"][0]["content"][0]["text"] = "Forged prompt"
                elif changed == "thread_id":
                    thread["id"] = "other-thread"
                elif changed == "turn_id":
                    turn["id"] = "other-turn"
                elif changed == "status":
                    turn["status"] = "failed"
                elif changed == "tool":
                    turn["items"].append({"type": "commandExecution"})
                elif changed == "missing_user":
                    turn["items"].pop(0)
                else:
                    turn["itemsView"] = "summary"
                self.assertEqual(self.observer.run_once(self.case).status, "configured_route_rejected")

    def test_reroute_event_preserved_and_never_actual_telemetry(self):
        reroute = {"method": "model/rerouted", "params": {"threadId": "thread-1", "turnId": "turn-1",
                    "fromModel": "gpt-6.1-sol", "toModel": "other", "reason": "highRiskCyberActivity"}}
        self.rpc.notifications.append(reroute)
        receipt = self.observer.run_once(self.case)
        self.assertEqual(receipt.status, "configured_route_rejected")
        self.assertIn(reroute, [event.get("notification") for event in json.loads(receipt.events_json)])
        self.assertIsNone(receipt.observed_actual)

    def test_late_reroute_during_seal_rejects_configured_route(self):
        reroute = {"method": "model/rerouted", "params": {"threadId": "thread-1", "turnId": "turn-1",
                    "fromModel": "gpt-6.1-sol", "toModel": "other", "reason": "highRiskCyberActivity"}}
        self.rpc.during_seal.append(reroute)

        receipt = self.observer.run_once(self.case)

        self.assertEqual(receipt.status, "configured_route_rejected")
        self.assertEqual(receipt.failure, "host reported model reroute")
        self.assertFalse(receipt.notification_capture_complete)
        self.assertIn(reroute, [event.get("notification") for event in json.loads(receipt.events_json)])

    def test_duplicate_completion_in_sealed_stream_rejects_acceptance(self):
        self.rpc.extra_sealed_notifications.append(deepcopy(self.rpc.completed))

        receipt = self.observer.run_once(self.case)

        self.assertEqual(receipt.status, "configured_route_rejected")
        self.assertIn("unique observed turn completion", receipt.failure)
        self.assertFalse(receipt.notification_capture_complete)

    def test_failed_stream_seal_rejects_and_consumes_invocation_key(self):
        self.rpc.seal_error = TimeoutError("fixture stream did not close")

        receipt = self.observer.run_once(self.case)

        self.assertEqual(receipt.status, "configured_route_rejected")
        self.assertIn("complete notification stream capture failed (TimeoutError)", receipt.failure)
        self.assertFalse(receipt.notification_capture_complete)
        self.assertEqual(self.rpc.seal_calls, [30])
        calls = list(self.rpc.calls)
        with self.assertRaisesRegex(ObservationRejected, "already reserved"):
            self.offline_observer().run_once(self.case)
        self.assertEqual(self.rpc.calls, calls)
        self.assertEqual(self.rpc.seal_calls, [30])

    def test_unknown_turn_send_consumes_key_and_never_retries(self):
        self.rpc.fail_method = "turn/start"
        receipt = self.observer.run_once(self.case)
        self.assertEqual(receipt.status, "unknown_after_reservation")
        self.assertIsNone(receipt.turn_id)
        calls = list(self.rpc.calls)
        with self.assertRaisesRegex(ObservationRejected, "already reserved"):
            self.observer.run_once(self.case)
        self.assertEqual(self.rpc.calls, calls)
        self.assertEqual(sum(method == "turn/start" for method, _ in calls), 1)

    def test_terminal_reuse_only_same_live_object_never_disk_authentication(self):
        receipt = self.observer.run_once(self.case)
        calls = list(self.rpc.calls)
        self.assertIs(self.observer.run_once(self.case), receipt)
        self.assertEqual(self.rpc.calls, calls)
        with self.assertRaisesRegex(ObservationRejected, "already reserved"):
            self.offline_observer().run_once(self.case)
        other_prompt = "Different exact prompt"
        selection = replace(self.case.selection, task_input_digest=_sha(other_prompt))
        with self.assertRaisesRegex(ObservationRejected, "conflicts"):
            self.observer.run_once(replace(self.case, prompt=other_prompt, selection=selection))

    def test_ledger_boundary_rejects_public_mode_and_symlinks(self):
        self.ledger.mkdir(mode=0o755)
        with self.assertRaisesRegex(ObservationRejected, "owner-only"):
            self.observer.run_once(self.case)
        self.assertEqual(self.rpc.calls, [])
        self.ledger.rmdir()
        self.ledger.symlink_to(self.ledger.parent, target_is_directory=True)
        with self.assertRaises(OSError):
            self.observer.run_once(self.case)
        self.assertEqual(self.rpc.calls, [])


if __name__ == "__main__":
    unittest.main()
