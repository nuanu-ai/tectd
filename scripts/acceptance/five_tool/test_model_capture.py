from __future__ import annotations

import hashlib
import json
import pathlib
import tempfile
import unittest
from unittest import mock

import model_capture
import run


def snapshot(thread_id: str, parent_id: str | None, items: list[dict]) -> dict:
    return {
        "metadata": {"thread": {
            "id": thread_id, "parentThreadId": parent_id, "model": "gpt-5.6-sol",
            "reasoningEffort": "medium", "ephemeral": True, "status": "completed",
        }},
        "item_pages": [{"response": {"data": [
            {"turnId": "turn", "item": item} for item in items
        ]}}],
    }


class ModelCaptureTests(unittest.TestCase):
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
            raw.append(first)
            first_bytes = raw.path.read_bytes()
            self.assertEqual(len(first_bytes.splitlines()), 1)
            raw.append(second)
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
            "lineage": {
                "parent": snapshot(parent_id, None, [spawn]), "observed_child_ids": [child_id],
                "child": child, "child_metadata_observations": [{
                    "id": child_id, "parentThreadId": parent_id, "model": "gpt-5.6-sol",
                    "reasoningEffort": "medium",
                }],
            },
        }
        evidence = model_capture.assert_one_sol_child(capture, parent_id)
        self.assertEqual(evidence["child_thread_id"], child_id)
        self.assertFalse(evidence["configured_model_is_per_turn_telemetry"])
        model_capture.assert_parent_boundary(capture)
        model_capture.assert_child_item_boundary(model_capture.child_items(capture))

        capture["observed_actor_thread_ids"].append("outside")
        with self.assertRaises(AssertionError):
            model_capture.assert_parent_boundary(capture)
        capture["observed_actor_thread_ids"].pop()
        capture["lineage"]["child"] = snapshot(child_id, parent_id, [{
            "id": "grandchild", "type": "collabAgentToolCall", "tool": "spawnAgent",
            "receiverThreadIds": ["grandchild"],
        }])
        with self.assertRaises(AssertionError):
            model_capture.assert_one_sol_child(capture, parent_id)

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


if __name__ == "__main__":
    unittest.main()
