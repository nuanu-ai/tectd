from __future__ import annotations
import json
import unittest
from protocol_compat import parse_delegation_features, wait_mcp_ready, validate_catalog, validate_get_state, PUBLIC_TOOLS

ID = "019d1a91-2acb-7c57-84d9-634b151171de"

class App:
    def __init__(self, events): self.notifications = list(events)
    def _read(self, timeout): raise TimeoutError("no ready notification")

def event(status, thread=ID):
    return {"method":"mcpServer/startupStatus/updated", "params":{"name":"tectd","threadId":thread,"status":status}}

class ProtocolTests(unittest.TestCase):
    def test_feature_stages_can_have_multiple_words_but_boolean_is_strict(self):
        output="multi_agent stable false\nmulti_agent_v2 under development false\n"
        self.assertEqual(parse_delegation_features(output, False), {"multi_agent":False,"multi_agent_v2":False})
        for bad in ["multi_agent stable false", output+"multi_agent stable false\n", output.replace("development false","development nope"), output.replace("stable false","stable true")]:
            with self.subTest(bad=bad), self.assertRaises(AssertionError): parse_delegation_features(bad, False)
    def test_ready_must_belong_to_same_server_native_thread(self):
        ready=event("ready")
        self.assertEqual(wait_mcp_ready(App([event("ready","wrong"),event("starting"),ready]),ID),ready)
        for bad in ["failed","cancelled","unknown",None]:
            with self.subTest(status=bad), self.assertRaises(AssertionError): wait_mcp_ready(App([event(bad)]),ID)
        with self.assertRaises(TimeoutError): wait_mcp_ready(App([event("ready","wrong")]),ID)
    def test_catalog_keeps_old_explicit_runtime_gate_and_exact_tools(self):
        server={"tools":{x:{} for x in PUBLIC_TOOLS}}
        validate_catalog(server)
        validate_catalog({**server,"runtimeStatus":"connected"})
        for bad in [{**server,"runtimeStatus":"failed"},{"tools":{"get_state":{}}},{"tools":{**server["tools"],"foreign":{}}},{"tools":[] }]:
            with self.assertRaises(AssertionError): validate_catalog(bad)
    def test_real_call_result_requires_canonical_state_and_correct_observable_identity(self):
        def result(payload): return {"content":[{"type":"text","text":"State"},{"type":"text","text":json.dumps(payload)},{"type":"text","text":"TECTD RESPONSE RULES"}]}
        validate_get_state(result({"status":"uninitialized"}),ID)
        validate_get_state(result({"status":"ready","session":{"native_session_id":ID}}),ID)
        for bad in [result({"status":"ready","session":{"native_session_id":"foreign"}}),result({"status":"broken"}),{"content":[]},{**result({"status":"ready"}),"isError":True}]:
            with self.assertRaises(AssertionError): validate_get_state(bad,ID)
        with self.assertRaises(AssertionError): validate_get_state(result({"status":"ready"}),"fake")
        for index, text in [(0, ""), (0, "x" * 2001), (2, ""), (2, "corrupt rules")]:
            bad = result({"status":"ready"}); bad["content"][index]["text"] = text
            with self.subTest(index=index, text=text[:20]), self.assertRaises(AssertionError): validate_get_state(bad, ID)
        with self.assertRaises(AssertionError): validate_get_state({**result({"status":"ready"}), "structuredContent": {}}, ID)

if __name__=="__main__": unittest.main()
