"""Offline test composition only: fake records are not authenticated evidence."""

import json
import tempfile
import unittest
from copy import deepcopy
from dataclasses import FrozenInstanceError, replace, asdict
from pathlib import Path
from unittest.mock import patch

from scripts.codex_app_server_observer import (
    AppServerObserver, CallerExecutionIntent, CallerRouteSelection, ExecutionIntent,
    ObservationRejected, _caller_intent_from_trusted_source, _mint_caller_source_ticket, _json, _sha,
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
        self.config = {"config": {"mcp_servers": {}, "plugins": {}, "web_search": "disabled",
                       "features": {flag: False for flag in ("plugins", "remote_plugin", "apps",
                       "shell_tool", "unified_exec", "multi_agent", "multi_agent_v2")}}}
        self.features = {"data": [{"name": name, "enabled": False, "stage": "stable"}
                                 for name in self.config["config"]["features"]], "nextCursor": None}
        self.pages = {}
        self.started = {"model": "gpt-6.1-sol", "modelProvider": "openai", "reasoningEffort": "medium",
                        "approvalPolicy": "never", "cwd": CWD, "sandbox": {"type": "readOnly"},
                        "thread": {"id": "thread-1", "ephemeral": True}}
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
        if method in self.pages:
            return deepcopy(self.pages[method][params.get("cursor")])
        return deepcopy({"model/list": self.models, "mcpServerStatus/list": self.mcp,
                         "experimentalFeature/list": self.features,
                         "config/read": self.config,
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
    def caller_intent(self, source="trusted-source-wiring-logic-only", observer=None):
        # Deliberate PRIVATE source-composition seam, never production authority.
        observer = observer or self.observer
        if observer._caller_source_issuer is None:
            observer._caller_source_issuer = object()
        selected = CallerRouteSelection("selected", "openai", "gpt-6.1-sol", "medium")
        requested = CallerRouteSelection("requested", "openai", "gpt-6-luna", "xhigh")
        recommended = CallerRouteSelection("recommended", "openai", "gpt-6.1-sol", "medium")
        material = _json({"offline_fixture": source, "input_sha256": _sha(PROMPT), "invocation_key": "fresh-s05-key",
            "selected_route": asdict(selected), "configured_route": asdict(selected),
            "requested_route": asdict(requested), "recommended_route": asdict(recommended)})
        ticket = _mint_caller_source_ticket(observer=observer, issuer=observer._caller_source_issuer,
            selection=selected, requested=requested, recommended=recommended, invocation_key="fresh-s05-key",
            input_sha256=_sha(PROMPT), source_binding_json=material, source_binding_digest=_sha(material))
        return _caller_intent_from_trusted_source(source_ticket=ticket, prompt=PROMPT, cwd=CWD)

    def test_caller_public_constructor_and_unsealed_values_deny(self):
        with self.assertRaises(ObservationRejected):
            CallerExecutionIntent(accepted=True)
        forged = object.__new__(CallerExecutionIntent)
        with self.assertRaises(ObservationRejected):
            self.observer.run_once(forged)
        self.assertEqual(self.rpc.calls, [])
        with self.assertRaises(ObservationRejected):
            _caller_intent_from_trusted_source(source_ticket={"accepted": True}, prompt=PROMPT, cwd=CWD)

    def test_private_caller_persisted_binding_and_direct_replay(self):
        case = self.caller_intent()
        self.rpc.started["thread"]["ephemeral"] = False
        receipt = self.observer.run_once(case)
        self.assertEqual(receipt.status, "completed_configured_route")
        self.assertIs(self.observer.run_once(case), receipt)
        self.assertIsNone(receipt.observed_actual)
        self.assertEqual(receipt.requested.route_id, "requested")
        self.assertEqual(receipt.recommended.route_id, "recommended")
        self.assertEqual(receipt.selection.route_id, "selected")
        self.assertEqual(receipt.source_binding_json, case.source_binding_json)
        self.assertEqual(receipt.source_binding_digest, case.source_binding_digest)
        self.assertIs(dict(self.rpc.calls)["thread/start"]["ephemeral"], False)
        reservation = json.loads((self.ledger / (_sha(case.invocation_key) + ".00-reserved.json")).read_text())
        self.assertEqual(reservation["source_binding_json"], case.source_binding_json)
        self.assertEqual(reservation["intent_digest"], case.digest)
        before = list(self.rpc.calls)
        reopened = self.offline_observer()
        with self.assertRaisesRegex(ObservationRejected, "already reserved"):
            reopened.run_once(self.caller_intent(observer=reopened))
        self.assertEqual(self.rpc.calls, before)
        reopened = self.offline_observer()
        with self.assertRaisesRegex(ObservationRejected, "conflicts"):
            reopened.run_once(self.caller_intent("changed-binding", observer=reopened))
        self.assertEqual(self.rpc.calls, before)

    def test_caller_unknown_send_and_resumed_reservation_never_retry(self):
        case = self.caller_intent()
        self.rpc.started["thread"]["ephemeral"] = False
        self.rpc.fail_method = "turn/start"
        receipt = self.observer.run_once(case)
        self.assertEqual(receipt.status, "unknown_after_reservation")
        before = list(self.rpc.calls)
        for observer in (self.observer, self.offline_observer()):
            with self.assertRaisesRegex(ObservationRejected, "already reserved"):
                observer.run_once(self.caller_intent(observer=observer))
        self.assertEqual(self.rpc.calls, before)

    def test_caller_reservation_symlink_and_bad_permissions_fail_closed(self):
        case = self.caller_intent()
        self.ledger.mkdir(mode=0o700)
        reservation = self.ledger / (_sha(case.invocation_key) + ".00-reserved.json")
        reservation.symlink_to(self.ledger / "missing-reservation")
        with self.assertRaisesRegex(ObservationRejected, "invalid reservation"):
            self.observer.run_once(case)
        self.assertEqual(self.rpc.calls, [])
        reservation.unlink()
        reservation.write_text(json.dumps({"intent_digest": case.digest}))
        reservation.chmod(0o644)
        with self.assertRaisesRegex(ObservationRejected, "invalid reservation"):
            self.observer.run_once(case)
        self.assertEqual(self.rpc.calls, [])

    def test_one_off_exact_configuration_gates_and_durable_replay(self):
        from scripts.codex_app_server_one_off_case import prepare_one_off_case
        case = prepare_one_off_case(CWD).intent
        self.rpc.models["data"][0]["model"] = case.selection.model
        self.rpc.models["data"][0]["supportedReasoningEfforts"][0]["reasoningEffort"] = case.selection.effort
        self.rpc.started.update(model=case.selection.model, reasoningEffort=case.selection.effort)
        user = self.rpc.read["thread"]["turns"][0]["items"][0]
        user["clientId"] = case.invocation_key
        user["content"][0]["text"] = case.prompt
        receipt = self.observer.run_once(case, completion_timeout=180)
        self.assertEqual(receipt.status, "completed_configured_route")
        self.assertIsNone(receipt.observed_actual)
        methods = [method for method, _ in self.rpc.calls]
        self.assertLess(methods.index("model/list"), methods.index("thread/start"))
        self.assertLess(methods.index("config/read"), methods.index("thread/start"))
        self.assertLess(methods.index("mcpServerStatus/list"), methods.index("thread/start"))
        self.assertEqual(methods.count("thread/start"), 1)
        self.assertEqual(methods.count("turn/start"), 1)
        self.assertIs(self.observer.run_once(case, completion_timeout=180), receipt)
        with self.assertRaises(ObservationRejected):
            self.offline_observer().run_once(case, completion_timeout=180)
        with self.assertRaises(ObservationRejected):
            self.observer.run_once(case, completion_timeout=181)

    def test_one_off_failed_model_or_surface_gate_never_starts_thread(self):
        from scripts.codex_app_server_one_off_case import prepare_one_off_case
        case = prepare_one_off_case(CWD).intent
        for failure in ("model", "features", "plugins", "mcp"):
            with self.subTest(failure=failure), tempfile.TemporaryDirectory() as temporary:
                rpc = OfflineRpc()
                rpc.models["data"][0]["model"] = case.selection.model
                rpc.models["data"][0]["supportedReasoningEfforts"][0]["reasoningEffort"] = case.selection.effort
                if failure == "model": rpc.models["data"] = []
                if failure == "features": rpc.config["config"].pop("features")
                if failure == "plugins": rpc.config["config"].pop("plugins")
                if failure == "mcp": rpc.mcp["data"] = [{"name": "active"}]
                observer = object.__new__(AppServerObserver)
                observer._configure(rpc, Path(temporary) / "ledger", "offline_fixture")
                receipt = observer.run_once(case, completion_timeout=180)
                self.assertEqual(receipt.status, "configured_route_rejected")
                self.assertNotIn("thread/start", [method for method, _ in rpc.calls])
                self.assertNotIn("turn/start", [method for method, _ in rpc.calls])

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

    def test_six_disabled_configured_metadata_entries_accepted(self):
        names = [f"disabled-{index}" for index in range(6)]
        self.rpc.config["config"]["mcp_servers"] = {name: {"enabled": False} for name in names}
        self.rpc.config["config"]["plugins"] = {"fixture-plugin": {"enabled": False}}
        self.rpc.mcp["data"] = [{"name": name, "authStatus": "unsupported", "tools": {},
                                 "resources": [], "resourceTemplates": [], "serverInfo": None}
                                for name in names]
        self.assertEqual(self.observer.run_once(self.case).status, "completed_configured_route")
        for method, params in self.rpc.calls:
            if method == "mcpServerStatus/list":
                self.assertEqual(params["detail"], "full")

    def test_resolved_features_checked_on_same_rpc_before_thread_and_turn(self):
        receipt = self.observer.run_once(self.case)
        self.assertEqual(receipt.status, "completed_configured_route")
        calls = self.rpc.calls
        inventory = [(index, params) for index, (method, params) in enumerate(calls)
                     if method == "experimentalFeature/list"]
        self.assertEqual(len(inventory), 2)
        self.assertNotIn("threadId", inventory[0][1])
        self.assertEqual(inventory[1][1]["threadId"], "thread-1")
        self.assertLess(inventory[0][0], next(index for index, (method, _) in enumerate(calls)
                                           if method == "thread/start"))
        self.assertLess(inventory[1][0], next(index for index, (method, _) in enumerate(calls)
                                           if method == "turn/start"))

    def test_raw_false_resolved_true_or_absent_never_starts_thread(self):
        for flag in self.rpc.config["config"]["features"]:
            for change in ("enabled", "absent"):
                with self.subTest(flag=flag, change=change):
                    self.setUp()
                    if change == "enabled":
                        next(row for row in self.rpc.features["data"] if row["name"] == flag)["enabled"] = True
                    else:
                        self.rpc.features["data"] = [row for row in self.rpc.features["data"] if row["name"] != flag]
                    self.assertIs(self.rpc.config["config"]["features"][flag], False)
                    receipt = self.observer.run_once(self.case)
                    self.assertEqual(receipt.status, "configured_route_rejected")
                    self.assertNotIn("thread/start", [method for method, _ in self.rpc.calls])
                    self.assertNotIn("turn/start", [method for method, _ in self.rpc.calls])

    def test_resolved_malformed_duplicate_or_nonboolean_fail_closed(self):
        for change in ("duplicate", "nonbool", "null", "missing_enabled", "empty_name",
                       "padded_name", "object_name", "row", "stage", "data", "cursor_missing", "oversize"):
            with self.subTest(change=change):
                self.setUp()
                row = self.rpc.features["data"][0]
                if change == "duplicate":
                    self.rpc.features["data"].append(deepcopy(row))
                elif change == "nonbool": row["enabled"] = 0
                elif change == "null": row["enabled"] = None
                elif change == "missing_enabled": del row["enabled"]
                elif change == "empty_name": row["name"] = ""
                elif change == "padded_name": row["name"] = " plugins "
                elif change == "object_name": row["name"] = {}
                elif change == "row": self.rpc.features["data"][0] = []
                elif change == "stage": row["stage"] = "unknown"
                elif change == "data": self.rpc.features["data"] = {}
                elif change == "cursor_missing": del self.rpc.features["nextCursor"]
                else: self.rpc.features["data"] *= 15
                receipt = self.observer.run_once(self.case)
                self.assertEqual(receipt.status, "configured_route_rejected")
                self.assertNotIn("thread/start", [method for method, _ in self.rpc.calls])

    def test_resolved_feature_pagination_complete_and_unique(self):
        first, rest = self.rpc.features["data"][:3], self.rpc.features["data"][3:]
        self.rpc.pages["experimentalFeature/list"] = {
            None: {"data": first, "nextCursor": "features-2"},
            "features-2": {"data": rest, "nextCursor": None}}
        self.assertEqual(self.observer.run_once(self.case).status, "completed_configured_route")
        calls = [params for method, params in self.rpc.calls if method == "experimentalFeature/list"]
        self.assertEqual(len(calls), 4)
        self.assertTrue(all(params["threadId"] == "thread-1" for params in calls[2:]))

    def test_resolved_feature_pagination_duplicate_incomplete_or_invalid_reject(self):
        for change in ("duplicate_row", "duplicate_cursor", "blank_cursor", "nonstring_cursor", "bounded_pages"):
            with self.subTest(change=change):
                self.setUp()
                if change == "duplicate_row":
                    self.rpc.pages["experimentalFeature/list"] = {
                        None: {"data": self.rpc.features["data"], "nextCursor": "features-2"},
                        "features-2": {"data": [self.rpc.features["data"][0]], "nextCursor": None}}
                elif change == "bounded_pages":
                    self.rpc.pages["experimentalFeature/list"] = {
                        None if index == 0 else str(index): {"data": [], "nextCursor": str(index + 1)}
                        for index in range(16)}
                else:
                    cursor = "repeated" if change == "duplicate_cursor" else " " if change == "blank_cursor" else 0
                    self.rpc.features["nextCursor"] = cursor
                    if change == "duplicate_cursor":
                        self.rpc.pages["experimentalFeature/list"] = {
                            None: {"data": [], "nextCursor": cursor},
                            cursor: {"data": [], "nextCursor": cursor}}
                self.assertEqual(self.observer.run_once(self.case).status, "configured_route_rejected")
                self.assertNotIn("thread/start", [method for method, _ in self.rpc.calls])

    def test_resolved_feature_transport_failure_timeout_and_unsupported_reject(self):
        from scripts.codex_app_server_rpc import AppServerRpcError
        for error in (TimeoutError("private sentinel"), AppServerRpcError("experimentalFeature/list", -32601),
                      RuntimeError("private sentinel")):
            with self.subTest(error_type=type(error).__name__):
                self.setUp()
                request = self.rpc.request
                def failing(method, params, timeout=30):
                    if method == "experimentalFeature/list":
                        raise error
                    return request(method, params, timeout=timeout)
                self.rpc.request = failing
                receipt = self.observer.run_once(self.case)
                self.assertEqual(receipt.status, "configured_route_rejected")
                self.assertNotIn("thread/start", [method for method, _ in self.rpc.calls])
                self.assertNotIn("private sentinel", receipt.events_json)

    def test_disabled_surface_total_deadline_bounds_feature_pages(self):
        self.rpc.pages["experimentalFeature/list"] = {
            None: {"data": [], "nextCursor": "features-2"}}
        with patch("scripts.codex_app_server_observer.time.monotonic", side_effect=[100, 100, 100, 130, 156]):
            receipt = self.observer.run_once(self.case)
        self.assertEqual(receipt.status, "configured_route_rejected")
        self.assertEqual(sum(method == "experimentalFeature/list" for method, _ in self.rpc.calls), 1)
        self.assertNotIn("thread/start", [method for method, _ in self.rpc.calls])

    def test_owned_run_deadline_is_not_reset_for_refreshed_thread_surface(self):
        clock = [100.0]
        timeouts = []
        request = self.rpc.request
        def advancing(method, params, timeout=30):
            timeouts.append((method, timeout))
            result = request(method, params, timeout=timeout)
            clock[0] += 9
            return result
        self.rpc.request = advancing
        with patch("scripts.codex_app_server_observer.time.monotonic", side_effect=lambda: clock[0]):
            receipt = self.observer.run_once(self.case)
        self.assertEqual(receipt.status, "configured_route_rejected")
        self.assertIn("thread/start", [method for method, _ in self.rpc.calls])
        self.assertNotIn("turn/start", [method for method, _ in self.rpc.calls])
        self.assertEqual(timeouts[-1], ("experimentalFeature/list", 1.0))

    def test_owned_run_deadline_checked_immediately_before_turn_send(self):
        clock = [100.0]
        request = self.rpc.request
        def expiring(method, params, timeout=30):
            result = request(method, params, timeout=timeout)
            if method == "mcpServerStatus/list" and "threadId" in params:
                clock[0] = 154.0
            return result
        self.rpc.request = expiring
        original = self.observer._disabled_surface
        def late_surface(*args, **kwargs):
            original(*args, **kwargs)
            if len(args) > 2:
                clock[0] = 156.0
        self.observer._disabled_surface = late_surface
        with patch("scripts.codex_app_server_observer.time.monotonic", side_effect=lambda: clock[0]):
            receipt = self.observer.run_once(self.case)
        self.assertEqual(receipt.status, "configured_route_rejected")
        self.assertIn("thread/start", [method for method, _ in self.rpc.calls])
        self.assertNotIn("turn/start", [method for method, _ in self.rpc.calls])
        self.assertFalse((self.ledger / (_sha(self.case.invocation_key) + ".01-before-turn.json")).exists())
        self.assertFalse(any(event.get("method") == "turn/start" for event in json.loads(receipt.events_json)))
        self.assertTrue((self.ledger / (_sha(self.case.invocation_key) + ".00-reserved.json")).exists())
        with self.assertRaisesRegex(ObservationRejected, "already reserved"):
            self.offline_observer().run_once(self.case)

    def test_expiry_during_before_turn_fence_io_preserves_intent_without_sending(self):
        from scripts.codex_app_server_observer import _Ledger
        clock = [100.0]
        write = _Ledger.write
        def expiring_write(ledger, name, record):
            result = write(ledger, name, record)
            if name.endswith(".01-before-turn.json"):
                clock[0] = 156.0
            return result
        with patch("scripts.codex_app_server_observer.time.monotonic", side_effect=lambda: clock[0]), \
                patch.object(_Ledger, "write", expiring_write):
            receipt = self.observer.run_once(self.case)
        self.assertEqual(receipt.status, "configured_route_rejected")
        self.assertTrue((self.ledger / (_sha(self.case.invocation_key) + ".01-before-turn.json")).exists())
        self.assertNotIn("turn/start", [method for method, _ in self.rpc.calls])
        self.assertFalse(any(event.get("method") == "turn/start" for event in json.loads(receipt.events_json)))

    def test_refreshed_thread_resolved_feature_enabled_never_starts_turn(self):
        request = self.rpc.request
        def refreshed(method, params, timeout=30):
            result = request(method, params, timeout=timeout)
            if method == "experimentalFeature/list" and "threadId" in params:
                next(row for row in result["data"] if row["name"] == "unified_exec")["enabled"] = True
            return result
        self.rpc.request = refreshed
        self.assertEqual(self.observer.run_once(self.case).status, "configured_route_rejected")
        self.assertIn("thread/start", [method for method, _ in self.rpc.calls])
        self.assertNotIn("turn/start", [method for method, _ in self.rpc.calls])

    def test_feature_metadata_sanitizes_private_values_and_raw_presence_types(self):
        secret = "PRIVATE_FEATURE_SENTINEL"
        self.rpc.features["data"].append({"name": secret, "enabled": False, "stage": "stable",
                                           "description": secret, "displayName": secret})
        receipt = self.observer.run_once(self.case)
        self.assertEqual(receipt.status, "completed_configured_route")
        self.assertNotIn(secret, receipt.events_json)
        for value, expected in ((None, "null"), ({"private": secret}, "object"), (0, "number")):
            self.setUp()
            self.rpc.config["config"]["features"]["multi_agent_v2"] = value
            receipt = self.observer.run_once(self.case)
            self.assertEqual(receipt.status, "configured_route_rejected")
            shape = next(event for event in json.loads(receipt.events_json)
                         if event.get("method") == "config/read")["effective_surface"]["feature_shape"]["multi_agent_v2"]
            self.assertEqual(shape, {"present": True, "json_type": expected, "boolean": None})
            self.assertNotIn(secret, receipt.events_json)
        self.setUp()
        del self.rpc.config["config"]["features"]["multi_agent_v2"]
        receipt = self.observer.run_once(self.case)
        shape = next(event for event in json.loads(receipt.events_json)
                     if event.get("method") == "config/read")["effective_surface"]["feature_shape"]["multi_agent_v2"]
        self.assertEqual(shape, {"present": False, "json_type": "missing", "boolean": None})

    def test_disabled_metadata_unknown_active_missing_or_duplicate_reject(self):
        changes = ("enabled", "unknown", "tools", "resources", "templates", "serverInfo",
                   "auth", "missing_auth", "missing_tools", "unreported", "duplicate", "missing_config",
                   "missing_features", "plugin_enabled", "apps_enabled")
        for change in changes:
            with self.subTest(change=change):
                self.setUp()
                config = self.rpc.config["config"]
                config["mcp_servers"] = {"disabled": {"enabled": False}}
                row = {"name": "disabled", "authStatus": "unsupported", "tools": {},
                       "resources": [], "resourceTemplates": []}
                self.rpc.mcp["data"] = [row]
                if change == "enabled":
                    config["mcp_servers"]["disabled"]["enabled"] = True
                elif change == "unknown":
                    row["name"] = "unconfigured"
                elif change == "tools":
                    row["tools"] = {"tool": {}}
                elif change == "resources":
                    row["resources"] = [{}]
                elif change == "templates":
                    row["resourceTemplates"] = [{}]
                elif change == "serverInfo":
                    row["serverInfo"] = {"name": "running"}
                elif change == "auth":
                    row["authStatus"] = "bearerToken"
                elif change == "missing_auth":
                    del row["authStatus"]
                elif change == "missing_tools":
                    del row["tools"]
                elif change == "unreported":
                    self.rpc.mcp["data"] = []
                elif change == "duplicate":
                    self.rpc.mcp["data"].append(deepcopy(row))
                elif change == "missing_config":
                    del config["mcp_servers"]
                elif change == "missing_features":
                    del config["features"]
                elif change == "plugin_enabled":
                    config["plugins"] = {"active": {"enabled": True}}
                else:
                    config["features"]["apps"] = True
                self.assertEqual(self.observer.run_once(self.case).status, "configured_route_rejected")
                self.assertNotIn("thread/start", [method for method, _ in self.rpc.calls])

    def test_inventory_full_pagination_hidden_model_and_disabled_statuses(self):
        self.rpc.config["config"]["mcp_servers"] = {"a": {"enabled": False}, "b": {"enabled": False}}
        row = lambda name: {"name": name, "authStatus": "unsupported", "tools": {},
                            "resources": [], "resourceTemplates": []}
        self.rpc.pages = {"model/list": {None: {"data": [], "nextCursor": "model-page-2"},
                           "model-page-2": self.rpc.models}, "mcpServerStatus/list": {
                           None: {"data": [row("a")], "nextCursor": "mcp-page-2"},
                           "mcp-page-2": {"data": [row("b")], "nextCursor": None}}}
        self.rpc.models["data"][0]["hidden"] = True
        self.assertEqual(self.observer.run_once(self.case).status, "completed_configured_route")
        model_calls = [params for method, params in self.rpc.calls if method == "model/list"]
        self.assertEqual(len(model_calls), 2)
        self.assertTrue(all(params["includeHidden"] is True for params in model_calls))

    def test_pagination_duplicate_cursor_fails_closed(self):
        self.rpc.pages["model/list"] = {None: {"data": [], "nextCursor": "repeated"},
                                        "repeated": {"data": [], "nextCursor": "repeated"}}
        receipt = self.observer.run_once(self.case)
        self.assertEqual(receipt.status, "configured_route_rejected")
        self.assertNotIn("thread/start", [method for method, _ in self.rpc.calls])

    def test_rejected_config_values_never_persist_private_payloads(self):
        secret = "PRIVATE_CONFIG_SENTINEL"
        for field in ("mcp", "plugin", "feature", "web_search"):
            for value in ({"credential": secret}, [secret], secret):
                with self.subTest(field=field, value_type=type(value).__name__):
                    self.setUp()
                    config = self.rpc.config["config"]
                    if field == "mcp":
                        config["mcp_servers"] = {"disabled": {"enabled": value}}
                    elif field == "plugin":
                        config["plugins"] = {"disabled-plugin": {"enabled": value}}
                    elif field == "feature":
                        config["features"]["apps"] = value
                    else:
                        config["web_search"] = value
                    receipt = self.observer.run_once(self.case)
                    self.assertEqual(receipt.status, "configured_route_rejected")
                    self.assertNotIn("thread/start", [method for method, _ in self.rpc.calls])
                    self.assertNotIn(secret, receipt.events_json)
                    for record in self.ledger.glob("*.json"):
                        self.assertNotIn(secret, record.read_text())

    def test_rejected_mcp_inventory_never_persists_private_payloads(self):
        secret = "PRIVATE_MCP_SENTINEL"
        for field in ("tools", "resources", "resourceTemplates", "serverInfo", "authStatus"):
            with self.subTest(field=field):
                self.setUp()
                self.rpc.config["config"]["mcp_servers"] = {"disabled": {"enabled": False}}
                row = {"name": "disabled", "authStatus": "unsupported", "tools": {},
                       "resources": [], "resourceTemplates": [], "serverInfo": None}
                row[field] = {"description": secret, "url": secret} if field in {
                    "tools", "serverInfo"} else [secret] if field != "authStatus" else secret
                self.rpc.mcp["data"] = [row]
                receipt = self.observer.run_once(self.case)
                self.assertEqual(receipt.status, "configured_route_rejected")
                self.assertNotIn("thread/start", [method for method, _ in self.rpc.calls])
                self.assertNotIn(secret, receipt.events_json)
                for record in self.ledger.glob("*.json"):
                    self.assertNotIn(secret, record.read_text())

    def test_valid_mcp_and_plugin_identifiers_are_hashed_in_all_persisted_events(self):
        secret = "PRIVATE_IDENTIFIER_SENTINEL"
        self.rpc.config["config"]["mcp_servers"] = {secret: {"enabled": False}}
        self.rpc.config["config"]["plugins"] = {secret: {"enabled": False}}
        self.rpc.mcp["data"] = [{"name": secret, "authStatus": "unsupported", "tools": {},
                                 "resources": [], "resourceTemplates": [], "serverInfo": None}]

        receipt = self.observer.run_once(self.case)

        self.assertEqual(receipt.status, "completed_configured_route")
        self.assertEqual(receipt.evidence_kind, "offline_fixture")
        events = json.loads(receipt.events_json)
        self.assertNotIn(secret, receipt.events_json)
        digest = _sha(secret)
        for event in events:
            if event.get("method") == "config/read":
                surface = event["effective_surface"]
                self.assertEqual(surface["mcp_enabled"][0]["identifier_sha256"], digest)
                self.assertEqual(surface["plugin_enabled"][0]["identifier_sha256"], digest)
            if event.get("method") == "mcpServerStatus/list":
                self.assertEqual(event["inventory_shape"]["entries"][0]["name_sha256"], digest)
        for record in self.ledger.glob("*.json"):
            self.assertNotIn(secret, record.read_text())

    def test_invalid_mcp_row_name_persists_only_type_and_null_digest(self):
        secret = "PRIVATE_INVALID_NAME_SENTINEL"
        self.rpc.mcp["data"] = [{"name": {"credential": secret}, "authStatus": "unsupported",
                                 "tools": {}, "resources": [], "resourceTemplates": [], "serverInfo": None}]

        receipt = self.observer.run_once(self.case)

        self.assertEqual(receipt.status, "configured_route_rejected")
        self.assertNotIn("thread/start", [method for method, _ in self.rpc.calls])
        self.assertNotIn(secret, receipt.events_json)
        summaries = [entry for event in json.loads(receipt.events_json)
                     if event.get("method") == "mcpServerStatus/list"
                     for entry in event["inventory_shape"]["entries"]]
        self.assertEqual(summaries[0]["name_type"], "dict")
        self.assertIsNone(summaries[0]["name_sha256"])
        for record in self.ledger.glob("*.json"):
            self.assertNotIn(secret, record.read_text())

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
