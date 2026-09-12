from __future__ import annotations

import json
import pathlib
import subprocess
import sys
import tempfile
import unittest
import uuid

import mcp_wire_capture as wire
import scope_candidates


class RelayFixture:
    def __init__(self, directory: pathlib.Path, policy: dict, gate: dict | None = None):
        self.root = directory
        self.logs = directory / "logs"
        self.policy = directory / "policy.json"
        self.gate = directory / "gate.json"
        self.launcher = directory / "launcher.py"
        self.run_sh = directory / "run.sh"
        self.binary = directory / "tectd-mcp"
        self.received = directory / "received.jsonl"
        self.launcher.write_text("fixture launcher\n")
        self.binary.write_bytes(b"exact packaged binary")
        self.fake = directory / "fake.py"
        self.fake.write_text(
            "import json,pathlib,sys\n"
            f"out=pathlib.Path({str(self.received)!r})\n"
            "for raw in sys.stdin.buffer:\n"
            " out.open('ab').write(raw)\n"
            " value=json.loads(raw)\n"
            " if value['id']==2: response={'jsonrpc':'2.0','id':2,'error':{'code':-32001,'message':'late backend error'}}\n"
            " else: response={'jsonrpc':'2.0','id':value['id'],'result':{'content':[{'type':'text','text':'Привет 🌊'}],'isError':False}}\n"
            " sys.stdout.buffer.write((json.dumps(response,ensure_ascii=False,separators=(',',':'))+'\\n').encode());sys.stdout.buffer.flush()\n"
        )
        self.run_sh.write_text("#!/bin/sh\nexec python3 ./fake.py\n")
        self.run_sh.chmod(0o700)
        wire.write_private_json(self.policy, policy)
        if gate is not None:
            wire.write_private_json(self.gate, gate)

    def command(self, wait: float = 0.02) -> list[str]:
        relay = pathlib.Path(wire.__file__).resolve()
        return [sys.executable, str(relay), "--log-dir", str(self.logs),
                "--policy", str(self.policy), "--gate", str(self.gate),
                "--launcher", str(self.launcher), "--relay", str(relay),
                "--run-sh", str(self.run_sh), "--binary", str(self.binary),
                "--gate-wait-seconds", str(wait)]


def request(identifier: int, actor: str, tool: str = "get_state", arguments: dict | None = None) -> bytes:
    value = {"jsonrpc": "2.0", "id": identifier, "method": "tools/call", "params": {
        "name": tool, "arguments": arguments or {}, "_meta": {"threadId": actor},
    }}
    return (json.dumps(value, ensure_ascii=False, separators=(",", ":")) + "\n").encode()


class McpWireCaptureTests(unittest.TestCase):
    def test_global_sequence_orders_pairs_across_connections(self) -> None:
        with tempfile.TemporaryDirectory() as name:
            directory = pathlib.Path(name)
            first = wire.PrivateLog(directory, {}, "guarded")
            second = wire.PrivateLog(directory, {}, "guarded")
            actor = str(uuid.uuid4())
            raw_one, raw_two = request(1, actor), request(2, actor, "help", {"mode": "search"})
            first.request(raw_one, json.loads(raw_one), "guarded")
            second.request(raw_two, json.loads(raw_two), "guarded")
            first_response = b'{"jsonrpc":"2.0","id":1,"result":{}}\n'
            second_response = b'{"jsonrpc":"2.0","id":2,"result":{}}\n'
            first.response(first_response, json.loads(first_response), "guarded",
                           response_source="mcp_wire", forwarded=True)
            second.response(second_response, json.loads(second_response), "guarded",
                            response_source="mcp_wire", forwarded=True)
            first.close()
            second.close()
            pairs, errors = wire.tool_pairs(directory)
            self.assertFalse(errors)
            self.assertEqual([pair["request"]["id"] for pair in pairs], [1, 2])
            records = wire.read_records(directory)
            self.assertEqual([row["global_sequence"] for row in records], list(range(1, 7)))
            self.assertEqual({row["phase"] for row in records}, {"guarded"})

    def test_relay_preserves_unicode_bytes_pairs_results_and_backend_error(self) -> None:
        with tempfile.TemporaryDirectory() as name:
            fixture = RelayFixture(pathlib.Path(name), {"phase": "transparent"})
            actor = str(uuid.uuid4())
            first, second = request(1, actor), request(2, actor, "help", {"mode": "search", "text": "Δ"})
            run = subprocess.run(fixture.command(), input=first + second, stdout=subprocess.PIPE,
                                 stderr=subprocess.PIPE, check=True)
            responses = run.stdout.splitlines(keepends=True)
            self.assertEqual(len(responses), 2)
            self.assertIn("Привет 🌊".encode(), responses[0])
            self.assertIn(b"late backend error", responses[1])
            self.assertEqual(fixture.received.read_bytes(), first + second)
            pairs, errors = wire.tool_pairs(fixture.logs, phase="transparent")
            self.assertFalse(errors)
            self.assertEqual(len(pairs), 2)
            self.assertEqual(pairs[0]["request"], json.loads(first))
            self.assertEqual(pairs[0]["response"], json.loads(responses[0]))
            self.assertEqual(pairs[1]["response_source"], "mcp_wire")
            self.assertTrue(pairs[1]["forwarded"])
            self.assertEqual({pair["connection_id"] for pair in pairs}, {pairs[0]["connection_id"]})
            header = wire.read_records(fixture.logs)[0]["parsed"]
            self.assertEqual(header["packaged_binary"]["sha256"], wire.sha256_file(fixture.binary))
            failed = scope_candidates._wire_call(pairs[1])
            with self.assertRaises(AssertionError):
                scope_candidates.assert_successful_calls([failed], {"help"})

    def test_closed_gate_does_not_forward_command_and_marks_fixture_error(self) -> None:
        with tempfile.TemporaryDirectory() as name:
            parent, child = str(uuid.uuid4()), str(uuid.uuid4())
            fixture = RelayFixture(pathlib.Path(name), {"phase": "guarded", "parent_thread_id": parent})
            raw = request(3, child, "command", {"route": "scope.candidates.refresh", "params": {}})
            run = subprocess.run(fixture.command(), input=raw, stdout=subprocess.PIPE,
                                 stderr=subprocess.PIPE, check=True)
            self.assertFalse(fixture.received.exists())
            self.assertIn(b"native_capture_wait_for_identity_gate", run.stdout)
            pairs, errors = wire.tool_pairs(fixture.logs)
            self.assertFalse(errors)
            self.assertEqual(pairs[0]["response_source"], "fixture_capture")
            self.assertFalse(pairs[0]["forwarded"])
            response_rows = [row for row in wire.read_records(fixture.logs)
                             if row["direction"] == "response"]
            self.assertEqual(response_rows[0]["response_source"], "fixture_capture")
            self.assertFalse(response_rows[0]["forwarded"])
            self.assertEqual(scope_candidates._wire_call(pairs[0])["source"], "fixture_capture")

    def test_open_gate_forwards_only_matching_child_and_approved_route(self) -> None:
        with tempfile.TemporaryDirectory() as name:
            parent, child = str(uuid.uuid4()), str(uuid.uuid4())
            policy = {"phase": "guarded", "parent_thread_id": parent}
            gate = {"parent_thread_id": parent, "child_thread_id": child}
            fixture = RelayFixture(pathlib.Path(name), policy, gate)
            raw = request(4, child, "command", {"route": "scope.candidates.refresh", "params": {}})
            run = subprocess.run(fixture.command(), input=raw, stdout=subprocess.PIPE,
                                 stderr=subprocess.PIPE, check=True)
            self.assertEqual(fixture.received.read_bytes(), raw)
            self.assertIn("Привет 🌊".encode(), run.stdout)
            base = {"method": "tools/call", "params": {"name": "get_state", "arguments": {}}}
            self.assertEqual(wire.guarded_error(base, policy, gate), "invalid_native_actor")
            base["params"]["_meta"] = {"threadId": parent}
            self.assertEqual(wire.guarded_error(base, policy, gate), "parent_mcp_forbidden")
            base["params"]["_meta"] = {"threadId": str(uuid.uuid4())}
            self.assertEqual(wire.guarded_error(base, policy, gate), "actor_gate_mismatch")
            base["params"]["_meta"] = {"threadId": child}
            base["params"]["name"] = "execute"
            self.assertEqual(wire.guarded_error(base, policy, gate), "execute_forbidden")
            base["params"]["name"] = "unknown"
            self.assertEqual(wire.guarded_error(base, policy, gate), "forbidden_tool")
            for route in ["program.get", "source.list"]:
                query = {"method": "tools/call", "params": {
                    "name": "query", "arguments": {"route": route, "params": {}},
                    "_meta": {"threadId": child},
                }}
                self.assertIsNone(wire.guarded_error(query, policy, gate))
            select = {"method": "tools/call", "params": {
                "name": "command", "arguments": {
                    "route": "session.select_worktrees", "params": {"worktree_ids": [str(uuid.uuid4())]},
                }, "_meta": {"threadId": child},
            }}
            self.assertIsNone(wire.guarded_error(select, policy, gate))
            select["params"]["arguments"]["route"] = "source.register"
            self.assertEqual(wire.guarded_error(select, policy, gate), "forbidden_route")

    def test_duplicate_live_rpc_id_is_rejected_without_forwarding_or_overwriting(self) -> None:
        with tempfile.TemporaryDirectory() as name:
            fixture = RelayFixture(pathlib.Path(name), {"phase": "transparent"})
            fixture.fake.write_text(
                "import json,pathlib,sys,time\n"
                f"out=pathlib.Path({str(fixture.received)!r})\n"
                "for raw in sys.stdin.buffer:\n"
                " out.open('ab').write(raw);time.sleep(.1)\n"
                " value=json.loads(raw);response={'jsonrpc':'2.0','id':value['id'],'result':{'content':[]}}\n"
                " sys.stdout.buffer.write((json.dumps(response,separators=(',',':'))+'\\n').encode());sys.stdout.buffer.flush()\n"
            )
            actor = str(uuid.uuid4())
            first = request(7, actor)
            duplicate = request(7, actor, "help", {"mode": "search"})
            run = subprocess.run(fixture.command(), input=first + duplicate, stdout=subprocess.PIPE,
                                 stderr=subprocess.PIPE, check=True)
            self.assertEqual(fixture.received.read_bytes(), first)
            pairs, errors = wire.tool_pairs(fixture.logs, phase="transparent")
            self.assertFalse(errors)
            self.assertEqual(len(pairs), 2)
            self.assertEqual([pair["response_source"] for pair in pairs],
                             ["mcp_wire", "fixture_capture"])
            self.assertEqual(pairs[1]["response"]["error"]["message"],
                             "native_capture_duplicate_request_id")

    def test_reader_keeps_first_response_and_rejects_duplicate_or_early_response(self) -> None:
        with tempfile.TemporaryDirectory() as name:
            directory = pathlib.Path(name)
            log = wire.PrivateLog(directory, {}, "guarded")
            actor = str(uuid.uuid4())
            raw = request(8, actor)
            request_sequence, duplicate = log.request(raw, json.loads(raw), "guarded")
            self.assertFalse(duplicate)
            first = b'{"jsonrpc":"2.0","id":8,"result":{"first":true}}\n'
            second = b'{"jsonrpc":"2.0","id":8,"result":{"second":true}}\n'
            log.response(first, json.loads(first), "guarded", response_source="mcp_wire",
                         forwarded=True, request_sequence=request_sequence)
            log.response(second, json.loads(second), "guarded", response_source="mcp_wire",
                         forwarded=True, request_sequence=request_sequence)
            log.close()
            pairs, errors = wire.tool_pairs(directory)
            self.assertEqual(pairs[0]["response"]["result"], {"first": True})
            self.assertEqual([error["error"] for error in errors], ["duplicate MCP response"])

            path = next(directory.glob("mcp-wire-*.jsonl"))
            rows = [json.loads(line) for line in path.read_text().splitlines()]
            original_source = rows[2]["response_source"]
            rows[2]["response_source"] = None
            path.write_text("".join(json.dumps(row, separators=(",", ":")) + "\n" for row in rows))
            with self.assertRaises(AssertionError):
                wire.read_records(directory)
            rows[2]["response_source"] = original_source
            rows[0]["global_sequence"], rows[1]["global_sequence"], rows[2]["global_sequence"] = 2, 3, 1
            path.write_text("".join(json.dumps(row, separators=(",", ":")) + "\n" for row in rows))
            _, errors = wire.tool_pairs(directory)
            self.assertIn("MCP response precedes its request", [error["error"] for error in errors])


if __name__ == "__main__":
    unittest.main()
