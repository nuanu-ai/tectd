from __future__ import annotations

import hashlib
import json
import pathlib
import tempfile
import unittest
import uuid
from unittest import mock

import model_capture
import run


def snapshot(thread_id: str, parent_id: str | None, items: list[dict]) -> dict:
    return {
        "response": {"thread": {
            "id": thread_id, "parentThreadId": parent_id, "model": "gpt-5.6-sol",
            "reasoningEffort": "medium", "ephemeral": True, "status": "completed",
        }},
    }


class ModelCaptureTests(unittest.TestCase):
    def test_identity_gate_requires_exact_child_metadata_inventory_and_get_state(self) -> None:
        parent_id, child_id = str(uuid.uuid4()), str(uuid.uuid4())

        class App:
            child_model = "gpt-5.6-sol"
            parent_model = "gpt-5.6-sol"
            parent_id_override = None
            loaded = [parent_id, child_id]

            def request(self, method, params):
                if method == "thread/loaded/list":
                    return {"data": self.loaded, "nextCursor": None}
                if method == "thread/read":
                    identifier = params["threadId"]
                    model = self.child_model if identifier == child_id else self.parent_model
                    returned_id = self.parent_id_override or identifier
                    return {"thread": {"id": returned_id, "parentThreadId": parent_id if identifier == child_id else None,
                            "model": model, "reasoningEffort": "medium", "ephemeral": True,
                            "status": {"type": "active"}}}
                raise AssertionError(method)

        class Fixture:
            opened = None
            actor = child_id

            def guarded_wire_pairs(self, allow_pending=False):
                return [{"source": "mcp_wire", "response_source": "mcp_wire", "forwarded": True,
                    "connection_id": "connection", "request_sequence": 2,
                    "request_raw_sha256": "request", "response_raw_sha256": "response",
                    "request": {"method": "tools/call", "params": {"name": "get_state", "arguments": {},
                                "_meta": {"threadId": self.actor}}},
                    "response": {"result": {"content": [], "isError": False}}}], []

            def open_model_gate(self, parent, child):
                self.opened = (parent, child)

        capture = {"thread_id": parent_id, "lineage_event_items": [
            {"type": "subAgentActivity", "agentThreadId": child_id},
        ]}
        proof = mock.Mock()
        fixture, app = Fixture(), App()
        self.assertTrue(model_capture.try_open_identity_gate(app, parent_id, capture, proof, fixture))
        self.assertEqual(fixture.opened, (parent_id, child_id))
        app.loaded = [parent_id]
        self.assertTrue(model_capture.try_open_identity_gate(app, parent_id, capture, proof, fixture))
        app.loaded = [parent_id, child_id]
        app.child_model = "unexpected"
        with self.assertRaises(AssertionError):
            model_capture.try_open_identity_gate(app, parent_id, capture, proof, fixture)
        app.child_model = "gpt-5.6-sol"
        app.parent_model = "unexpected"
        with self.assertRaises(AssertionError):
            model_capture.try_open_identity_gate(app, parent_id, capture, proof, fixture)
        app.parent_model = "gpt-5.6-sol"
        app.parent_id_override = str(uuid.uuid4())
        with self.assertRaises(AssertionError):
            model_capture.try_open_identity_gate(app, parent_id, capture, proof, fixture)
        app.parent_id_override = None
        app.loaded.append(str(uuid.uuid4()))
        with self.assertRaises(AssertionError):
            model_capture.try_open_identity_gate(app, parent_id, capture, proof, fixture)
        app.loaded.pop()
        fixture.actor = str(uuid.uuid4())
        with self.assertRaises(AssertionError):
            model_capture.try_open_identity_gate(app, parent_id, capture, proof, fixture)
        fixture.guarded_wire_pairs = lambda allow_pending=False: ([{
            "source": "mcp_wire", "response_source": "fixture_capture", "forwarded": False,
            "request": {"method": "tools/call", "params": {"name": "get_state",
                "arguments": {}, "_meta": {"threadId": child_id}}},
            "response": {"error": {"code": -32600, "message": "not forwarded"}},
        }], [])
        with self.assertRaises(AssertionError):
            model_capture.try_open_identity_gate(app, parent_id, capture, proof, fixture)

    def test_delegation_overrides_are_explicit_per_mode(self) -> None:
        self.assertEqual(run.delegation_overrides(False), [
            "-c", "features.multi_agent=false", "-c", "features.multi_agent_v2=false",
        ])
        self.assertEqual(run.delegation_overrides(True), [
            "-c", "features.multi_agent=true", "-c", "features.multi_agent_v2=false",
        ])
        codex = pathlib.Path("/owned/codex")
        fixture = mock.Mock()
        fixture.package = pathlib.Path("/owned/package")
        fixture.launcher = pathlib.Path("/owned/launcher")
        fixture.daemon_socket = pathlib.Path("/owned/socket")
        fixture.host_config = pathlib.Path("/owned/host.json")
        fixture.workspace_key = "owned"
        with mock.patch.object(run.subprocess, "check_output", return_value=(
            "multi_agent experimental true\nmulti_agent_v2 experimental false\n"
        )) as check:
            self.assertEqual(run.delegation_features(codex, fixture, True), {
                "multi_agent": True, "multi_agent_v2": False,
            })
            command = check.call_args.args[0]
            self.assertIn("features.multi_agent=true", command)
            self.assertIn("features.multi_agent_v2=false", command)
        with mock.patch.object(run.subprocess, "check_output", return_value=(
            "multi_agent experimental true\nmulti_agent_v2 experimental false\n"
        )):
            with self.assertRaises(AssertionError):
                run.delegation_features(codex, fixture, False)

    def test_raw_events_are_ordered_append_only_and_hash_exactly(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            proof = pathlib.Path(directory) / "proof.json"
            raw = model_capture.RawEventLog(proof)
            first = {"method": "item/agentMessage/delta", "params": {"delta": "one"}}
            second = {"method": "item/completed", "params": {"item": {"id": "mcp", "type": "mcpToolCall"}}}
            with mock.patch.object(model_capture.os, "fsync", wraps=model_capture.os.fsync) as sync:
                raw.append(first)
                first_bytes = raw.path.read_bytes()
                self.assertEqual(len(first_bytes.splitlines()), 1)
                raw.append(second)
                self.assertEqual(sync.call_count, 2)
            evidence = raw.evidence()
            raw.close()
            content = pathlib.Path(evidence["path"]).read_bytes()
            rows = [json.loads(line) for line in content.splitlines()]
            self.assertEqual([row["sequence"] for row in rows], [1, 2])
            self.assertEqual([row["event"] for row in rows], [first, second])
            self.assertTrue(content.startswith(first_bytes))
            self.assertEqual(evidence["count"], 2)
            self.assertEqual(evidence["sha256"], hashlib.sha256(content).hexdigest())

    def test_lineage_guards_accept_one_child_and_reject_extra_activity(self) -> None:
        parent_id, child_id = "parent", "child"
        spawn = {"id": "spawn", "type": "collabAgentToolCall", "tool": "spawnAgent",
                 "senderThreadId": parent_id, "receiverThreadIds": [child_id],
                 "model": "gpt-5.6-sol", "reasoningEffort": "medium"}
        child = snapshot(child_id, parent_id, [{"id": "call", "type": "mcpToolCall"}])
        capture = {
            "thread_id": parent_id, "observed_actor_thread_ids": [parent_id, child_id],
            "lineage_event_items": [spawn, {"id": "activity", "type": "subAgentActivity",
                                                   "agentThreadId": child_id}],
            "lineage": {
                "parent": snapshot(parent_id, None, []), "observed_child_ids": [child_id],
                "child": child, "loaded_thread_ids": [parent_id, child_id], "child_metadata_observations": [{
                    "id": child_id, "parentThreadId": parent_id, "model": "gpt-5.6-sol",
                    "reasoningEffort": "medium",
                }], "parent_metadata_observations": [{
                    "id": parent_id, "parentThreadId": None, "model": "gpt-5.6-sol",
                    "reasoningEffort": "medium",
                }],
            },
        }
        evidence = model_capture.assert_one_sol_child(capture, parent_id)
        self.assertEqual(evidence["child_thread_id"], child_id)
        self.assertFalse(evidence["configured_model_is_per_turn_telemetry"])
        self.assertTrue(evidence["spawn_request_observed"])
        self.assertFalse(evidence["child_non_mcp_actions_observable"])
        model_capture.assert_parent_boundary(capture)

        capture["observed_actor_thread_ids"].append("outside")
        with self.assertRaises(AssertionError):
            model_capture.assert_parent_boundary(capture)
        capture["observed_actor_thread_ids"].pop()
        capture["lineage_event_items"].append({
            "id": "grandchild", "type": "collabAgentToolCall", "tool": "spawnAgent",
            "receiverThreadIds": ["grandchild"], "senderThreadId": child_id,
        })
        with self.assertRaises(AssertionError):
            model_capture.assert_parent_boundary(capture)

    def test_missing_spawn_item_is_recorded_without_weakening_child_metadata(self) -> None:
        parent_id, child_id = "parent", "child"
        capture = {"thread_id": parent_id, "lineage_event_items": [{
            "id": "activity", "type": "subAgentActivity", "agentThreadId": child_id,
        }], "lineage": {"observed_child_ids": [child_id],
            "parent": snapshot(parent_id, None, []),
            "child": snapshot(child_id, parent_id, []), "loaded_thread_ids": [parent_id],
            "child_metadata_observations": [{
                "id": child_id, "parentThreadId": parent_id, "model": "gpt-5.6-sol",
                "reasoningEffort": "medium",
            }], "parent_metadata_observations": [{
                "id": parent_id, "parentThreadId": None, "model": "gpt-5.6-sol",
                "reasoningEffort": "medium",
            }]}}
        evidence = model_capture.assert_one_sol_child(capture, parent_id)
        self.assertFalse(evidence["spawn_request_observed"])
        self.assertFalse(evidence["loaded_child_observed"])

    def test_raw_evidence_survives_a_later_guard_failure(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            raw = model_capture.RawEventLog(pathlib.Path(directory) / "proof.json")
            raw.append({"method": "turn/completed", "params": {"threadId": "unexpected"}})
            evidence = raw.evidence()
            raw.close()
            with self.assertRaises(AssertionError):
                model_capture.assert_parent_boundary({
                    "thread_id": "parent", "observed_actor_thread_ids": ["unexpected"],
                    "lineage": {"parent": snapshot("parent", None, []), "observed_child_ids": ["child"]},
                })
            self.assertEqual(len(pathlib.Path(evidence["path"]).read_text().splitlines()), 1)

    def test_wire_validation_failure_retains_private_artifact_manifest(self) -> None:
        fixture = mock.Mock()
        fixture.wire_evidence.side_effect = AssertionError("invalid global sequence")
        fixture.wire_artifacts.return_value = {
            "directory": "/private/owned-wire", "files": [{"sha256": "abc", "records": 3}],
        }
        proof = mock.Mock()
        proof.data = {"status": "running"}
        run.retain_wire_evidence(fixture, proof)
        self.assertEqual(proof.data["status"], "fail")
        self.assertEqual(proof.data["validation_errors"], [{
            "source": "fixture_capture", "stage": "mcp_wire_capture",
            "error": "AssertionError: invalid global sequence",
        }])
        self.assertEqual(proof.data["mcp_wire_capture"]["files"][0]["records"], 3)
        self.assertFalse(proof.data["mcp_wire_capture"]["chain_verified"])

    def test_real_collector_helper_records_and_rejects_parent_shell_item(self) -> None:
        parent_id, child_id = "parent", "child"
        for kind in ["commandExecution", "webSearch", "mcpToolCall"]:
            capture = {"thread_id": parent_id, "lineage_event_items": [],
                       "lineage": {"observed_child_ids": [child_id]}}
            event = {"method": "item/completed", "params": {"threadId": parent_id,
                "item": {"id": kind, "type": kind, "status": "completed",
                         "private": "intentionally not copied"}}}
            self.assertTrue(model_capture.record_parent_item(capture, parent_id, event))
            self.assertEqual(capture["lineage_event_items"], [{
                "id": kind, "type": kind, "status": "completed",
            }])
            with self.assertRaises(AssertionError):
                model_capture.assert_parent_boundary(capture)


if __name__ == "__main__":
    unittest.main()
