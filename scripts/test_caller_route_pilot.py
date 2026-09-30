"""Offline-only tests for the S05 caller dispatch pilot."""

import json
import os
import tempfile
import unittest
from datetime import datetime, timedelta, timezone
from pathlib import Path

from scripts.caller_route_pilot import (
    DispatchRejected, OfflineMockHost, OfflinePilotPermit, OfflineSelectionSource, dispatch_once,
)


NOW = datetime(2026, 9, 30, tzinfo=timezone.utc)
MODEL = "gpt-6-sol"
EFFORT = "medium"
ROUTE_B_MODEL = "gpt-6-luna"
ROUTE_B_EFFORT = "xhigh"
SURFACE = {"kind": "scripted_offline", "mcp_servers": [], "mcp_tool_ids": []}


def fixture():
    request = {"invocation_key": "task-123", "decision_id": "decision-123",
               "preparation_digest": "preparation-sha", "work_revision": "work-9",
               "capability_digest": "host-sha", "catalogue_version": "catalogue-2",
               "cwd": "/tmp/owned-fixture", "prompt": "Inspect this bounded fixture."}
    record = {"decision_id": "decision-123", "disposition": "accept", "advice_status": "selected",
              "preparation_digest": "preparation-sha", "work_revision": "work-9",
              "capability_digest": "host-sha", "catalogue_version": "catalogue-2",
              "requested_route": "route-a", "selected_route": "route-a",
              "model": MODEL, "effort": EFFORT,
              "expires_at": (NOW + timedelta(minutes=10)).isoformat()}
    routes = {"route-a": {"allowed": True, "catalogue_version": "catalogue-2",
                          "capability_digest": "host-sha", "model": MODEL, "effort": EFFORT},
              "route-b": {"allowed": True, "catalogue_version": "catalogue-2",
                          "capability_digest": "host-sha", "model": ROUTE_B_MODEL,
                          "effort": ROUTE_B_EFFORT}}
    return request, record, routes


def replies(*, wrong_metadata=False, lost=False, model=MODEL, effort=EFFORT):
    actual_model = "unexpected-model" if wrong_metadata else model
    metadata = {"thread": {"id": "thread-1", "model": actual_model, "reasoningEffort": effort,
                           "turns": [{"id": "turn-1", "status": "completed"}]}}
    return [("thread/start", TimeoutError("response lost") if lost else {"thread": {"id": "thread-1"}}),
            ("thread/read", metadata), ("turn/start", {"turn": {"id": "turn-1"}}),
            ("thread/read", metadata)]


class PilotTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.ledger = Path(self.temporary.name) / "ledger"
        self.request, self.record, self.routes = fixture()
        self.permit = OfflinePilotPermit(self.request["invocation_key"])

    def source(self):
        return OfflineSelectionSource({self.record["decision_id"]: self.record}, self.routes)

    def host(self, **kwargs):
        return OfflineMockHost(replies(**kwargs), tool_surface=SURFACE)

    def run_pilot(self, host=None):
        return dispatch_once(self.source(), host or self.host(), self.request, self.ledger,
                             self.permit, now=NOW)

    def stages(self):
        return [json.loads(path.read_text()) for path in sorted(self.ledger.glob("*.json"))]

    def test_exact_model_effort_separate_evidence_and_durable_stages(self):
        host = self.host()
        result = self.run_pilot(host)
        self.assertEqual([name for name, _ in host.calls],
                         ["thread/start", "thread/read", "turn/start", "thread/read"])
        self.assertEqual(host.calls[0][1]["model"], MODEL)
        self.assertEqual(host.calls[0][1]["reasoningEffort"], EFFORT)
        self.assertIs(host.calls[0][1]["allowProviderModelFallback"], False)
        self.assertEqual(host.calls[2][1]["model"], MODEL)
        self.assertEqual(host.calls[2][1]["effort"], EFFORT)
        self.assertEqual(result["status"], "started")
        self.assertEqual(result["requested_route"], "route-a")
        self.assertEqual(result["selected_route"], "route-a")
        self.assertEqual(result["dispatched_configured"]["model"], MODEL)
        self.assertIsNone(result["observed_actual"])
        self.assertEqual(result["turn_outcome"], "completed")
        stages = self.stages()
        self.assertEqual([item["stage"] for item in stages],
                         ["reserved", "thread_started", "metadata_checked", "turn_started", "outcome_read"])
        self.assertEqual(stages[1]["thread_id"], "thread-1")
        self.assertEqual(stages[3]["turn_id"], "turn-1")

    def test_forged_caller_fields_and_untrusted_client_never_dispatch(self):
        host = self.host()
        forged = dict(self.request, caller_validated=True, decision_status="accepted",
                      selected_route="route-b", selected_model=MODEL)
        forged["work_revision"] = "forged"
        with self.assertRaises(DispatchRejected):
            dispatch_once(self.source(), host, forged, self.ledger, self.permit, now=NOW)
        self.assertEqual(host.calls, [])
        with self.assertRaises(DispatchRejected):
            dispatch_once({"fake": True}, host, self.request, self.ledger, self.permit, now=NOW)
        self.assertEqual(host.calls, [])
        class LiveLikeHost:
            def request(self, method, params):
                raise AssertionError("must never call live host")
        with self.assertRaises(DispatchRejected):
            dispatch_once(self.source(), LiveLikeHost(), self.request, self.ledger,
                          self.permit, now=NOW)

    def test_unknown_or_tectd_mcp_surface_never_dispatch(self):
        for surface in ({}, {"kind": "scripted_offline", "mcp_servers": ["tectd"], "mcp_tool_ids": []},
                        {"kind": "scripted_offline", "mcp_servers": [], "mcp_tool_ids": ["tectd.command"]}):
            with self.subTest(surface=surface):
                host = OfflineMockHost(replies(), tool_surface=surface)
                with self.assertRaises(DispatchRejected):
                    self.run_pilot(host)
                self.assertEqual(host.calls, [])

    def test_abstain_stale_and_catalogue_mismatch_never_dispatch(self):
        for field, value in (("advice_status", "abstain"),
                             ("expires_at", (NOW - timedelta(seconds=1)).isoformat()),
                             ("catalogue_version", "old")):
            with self.subTest(field=field):
                record = dict(self.record, **{field: value})
                source = OfflineSelectionSource({record["decision_id"]: record}, self.routes)
                host = self.host()
                with self.assertRaises(DispatchRejected):
                    dispatch_once(source, host, self.request, self.ledger, self.permit, now=NOW)
                self.assertEqual(host.calls, [])

    def test_requested_selected_override_requires_authority(self):
        self.record["selected_route"] = "route-b"
        host = self.host()
        with self.assertRaisesRegex(DispatchRejected, "override"):
            self.run_pilot(host)
        self.assertEqual(host.calls, [])
        self.record["override_authorized"] = True
        self.record["model"] = ROUTE_B_MODEL
        self.record["effort"] = ROUTE_B_EFFORT
        host = self.host(model=ROUTE_B_MODEL, effort=ROUTE_B_EFFORT)
        result = self.run_pilot(host)
        self.assertEqual(result["requested_route"], "route-a")
        self.assertEqual(result["selected_route"], "route-b")
        self.assertEqual(host.calls[0][1]["model"], ROUTE_B_MODEL)
        self.assertEqual(host.calls[0][1]["reasoningEffort"], ROUTE_B_EFFORT)
        self.assertEqual(host.calls[2][1]["model"], ROUTE_B_MODEL)
        self.assertEqual(host.calls[2][1]["effort"], ROUTE_B_EFFORT)
        self.assertNotEqual((host.calls[0][1]["model"], host.calls[0][1]["reasoningEffort"]),
                            (MODEL, EFFORT))

    def test_selected_route_model_effort_mismatch_never_dispatch(self):
        self.record["selected_route"] = "route-b"
        self.record["override_authorized"] = True
        host = self.host()
        with self.assertRaisesRegex(DispatchRejected, "route model differs"):
            self.run_pilot(host)
        self.assertEqual(host.calls, [])

    def test_duplicate_and_lost_response_never_retry(self):
        host = self.host(lost=True)
        first = self.run_pilot(host)
        self.assertEqual(first["status"], "unknown_after_reservation")
        self.assertEqual([item["stage"] for item in self.stages()], ["reserved", "unknown"])
        with self.assertRaisesRegex(DispatchRejected, "already reserved"):
            self.run_pilot(host)
        self.assertEqual([name for name, _ in host.calls], ["thread/start"])

    def test_metadata_mismatch_blocks_turn(self):
        host = self.host(wrong_metadata=True)
        result = self.run_pilot(host)
        self.assertEqual(result["status"], "unknown_after_reservation")
        self.assertIn("configured model or effort mismatch", result["error"])
        self.assertEqual([name for name, _ in host.calls], ["thread/start", "thread/read"])
        self.assertIsNone(result["dispatched_configured"])

    def test_owner_only_ledger_and_symlink_rejection(self):
        self.ledger.mkdir(mode=0o755)
        host = self.host()
        with self.assertRaisesRegex(DispatchRejected, "owner-only"):
            self.run_pilot(host)
        self.assertEqual(host.calls, [])
        self.ledger.chmod(0o700)
        self.ledger.rmdir()
        self.ledger.symlink_to(Path(self.temporary.name), target_is_directory=True)
        with self.assertRaisesRegex(DispatchRejected, "symlink"):
            self.run_pilot(host)
        self.assertEqual(host.calls, [])


if __name__ == "__main__":
    unittest.main()
