"""Pure structural fixture tests. No native server, database or command proof."""
from __future__ import annotations
import copy
import hashlib
import json
from pathlib import Path
import unittest
import common
import native_slices
import pipeline_execution as pe

ROOT = Path(__file__).resolve().parents[3]
DEFINITIONS = ROOT / "crates/host/pipeline-definitions"


def definition(version=pe.LIGHTWEIGHT_VERSION):
    return json.loads((DEFINITIONS / f"lightweight-tdd-{version}.json").read_text())


def context(phase="K1", mode="whole", version=pe.LIGHTWEIGHT_VERSION):
    d = definition(version)
    return {"run":{"id":"11111111-1111-4111-8111-111111111111", "scope_id":"22222222-2222-4222-8222-222222222222",
                   "slice_id":"33333333-3333-4333-8333-333333333333", "revision":1,
                   "definition_kind":pe.LIGHTWEIGHT_KIND, "definition_version":version, "definition_digest":d["digest"],
                   "delivery_mode":mode,"status":"active","current_phase_id":phase,
                   "current_phase_ordinal":pe.LIGHTWEIGHT_PHASES.index(phase)+1},
            "definition":d, "delivered_phases":d["phases"] if mode=="whole" else [next(p for p in d["phases"] if p["id"]==phase)],
            "outputs":[],"bindings":[],"attempts":[],"inputs":[],"outputs_complete":True,"result":None}


def fixture_constraints(params, phase):
    """Independent structural checks against source constraints; not semantic QA."""
    output = params["output"]
    fields = output["fields"]
    assert all(isinstance(fields.get(k), str) and fields[k].strip() for k in phase["required_fields"])
    route = next(r for r in phase["verdict_routes"] if r["verdict"] == output["verdict"])
    assert (params["outcome"],params["transition"],output["dispositions"]) == (route["outcome"],route["transition"],route["dispositions"])
    for rule in phase["output_constraints"]:
        if rule.get("when_verdict") not in (None, output["verdict"]): continue
        value = fields[rule["field"]]
        if rule["kind"] == "field_equals": assert value == rule["value"]
        if rule["kind"] == "field_one_of": assert value in rule["values"]
        if rule["kind"] == "reviewer_context_mode":
            assert value == "self" and output.get("reviewer_context") is None
        if rule["kind"] == "command_receipt":
            receipt = json.loads(value)
            assert receipt["command"].startswith("STRUCTURAL_FIXTURE_NOT_EXECUTED:")
            assert isinstance(receipt["target"],str) and receipt["target"].strip()
            assert receipt["status"] == rule["required_status"]
            assert type(receipt["exit_code"]) is int
            assert (receipt["exit_code"] != 0) == rule["require_nonzero_exit"]
            assert receipt["fresh"] is True and receipt["skipped"] is False
            assert rule["required_scope"] in receipt["scopes"] and len(set(receipt["scopes"])) == len(receipt["scopes"])
            if rule.get("target_field"): assert receipt["target"] == fields[rule["target_field"]]


class FragmentTransport:
    """Actual host-shaped fragment wire fixtures, intentionally using unusual JSON spacing."""
    def __init__(self, value, params, page_bytes=97, mutate=None, terminal_actions=None):
        self.original = json.dumps(value, ensure_ascii=False, separators=(", ", ": ")).encode()
        self.params, self.page_bytes, self.mutate = copy.deepcopy(params), page_bytes, mutate
        self.digest = hashlib.sha256(self.original).hexdigest()
        self.calls = []
        self.terminal_actions = terminal_actions or []
    def __call__(self, tool, arguments):
        self.calls.append(copy.deepcopy((tool,arguments)))
        params = arguments["params"]
        start = params.get("offset_bytes",0)
        end = min(start+self.page_bytes,len(self.original))
        while True:
            try: text = self.original[start:end].decode("utf-8"); break
            except UnicodeDecodeError: end -= 1
        next_offset = end if end<len(self.original) else None
        source = {k:v for k,v in self.params.items() if k not in {"view","offset_bytes","limit_bytes","representation_digest","refresh"}}
        if self.params.get("view") == "output": source["view"]="output"
        actions = self.terminal_actions
        if next_offset is not None:
            continuation = {**self.params, "offset_bytes":next_offset, "limit_bytes":self.params.get("limit_bytes",4096),
                            "representation_digest":self.digest,"refresh":False}
            actions = [{"kind":"ready_call","tool":"query","arguments":{"route":"slice.pipeline.context","params":continuation}}]
        payload = {"kind":"fragment","format":"json","encoding":"utf-8","source":source,
                   "representation_digest":self.digest,"total_bytes":len(self.original),"offset_bytes":start,
                   "returned_bytes":end-start,"text":text,"next_offset_bytes":next_offset,"actions":copy.deepcopy(actions)}
        if self.mutate: self.mutate(payload,len(self.calls))
        return payload, False


class FakePipeline:
    """A simulated fixture transport; this is never a database acceptance oracle."""
    def __init__(self, ctx=None):
        self.ctx = copy.deepcopy(ctx or context())
        self.calls, self.history, self.reads = [], [], []
    def compact(self, key=None, result=None):
        run = copy.deepcopy(self.ctx["run"])
        compact = {"run":run,"definition":{k:self.ctx["definition"][k] for k in ("kind","version","digest")},
                   "delivery_scope":"snapshot_reference", "delivery_receipt":{"manifest_digest":run["definition_digest"],"context_epoch":run["revision"]},
                   "output_availability":{"complete":True,"view":"details","section":"outputs"},
                   "field_destinations":{"definition":{"view":"snapshot"}}}
        selectors = [{"run_id":run["id"],"view":"snapshot","definition_digest":run["definition_digest"]},
                     {"run_id":run["id"],"view":"details","run_revision":run["revision"],"section":"all"}]
        if run["current_phase_id"]:
            selectors.append({"run_id":run["id"],"view":"phase_contract","definition_digest":run["definition_digest"],"phase_id":run["current_phase_id"]})
        payload = {key:compact} if key else compact
        payload["actions"] = [{"kind":"ready_call","tool":"query","arguments":{"route":"slice.pipeline.context","params":p}} for p in selectors]
        if key=="context": payload["result_reference"]={"result_id":result["id"] if result else None,"view":"details","section":"history"}
        return payload
    def __call__(self, tool, arguments):
        route, p = arguments["route"],arguments["params"]
        self.calls.append(copy.deepcopy((tool,arguments)))
        run = self.ctx["run"]
        if route=="slice.pipeline.begin":
            version=p.get("definition_version",pe.LIGHTWEIGHT_VERSION)
            if version not in pe.LIGHTWEIGHT_PINS: return {"error":{"code":"LEGACY_MIGRATION_REQUIRED"}},True
            self.ctx=context(mode=p.get("delivery_mode","phasewise"),version=version)
            return self.compact("created"),False
        if tool=="query" and route=="slice.pipeline.context":
            view=p["view"]
            self.reads.append(view)
            if p["run_id"]!=run["id"]: return {"error":{"code":"not_found"}},True
            if view in {"snapshot","phase_contract"} and p["definition_digest"]!=run["definition_digest"]: return {"error":{"code":"METHOD_VERSION_UNAVAILABLE"}},True
            if view=="snapshot": value={"run_id":run["id"],"definition_digest":run["definition_digest"],"definition":self.ctx["definition"]}
            elif view=="phase_contract": value={"run_id":run["id"],"definition_digest":run["definition_digest"],"phase":next(x for x in self.ctx["definition"]["phases"] if x["id"]==p["phase_id"])}
            elif view=="details":
                if p["run_revision"]!=run["revision"]: return {"error":{"code":"STALE_REVISION"}},True
                data={k:copy.deepcopy(self.ctx.get(k)) for k in ("outputs","bindings","attempts","inputs","outputs_complete","result")}
                data.update({"delivered_phases":self.ctx["definition"]["phases"] if run["delivery_mode"]=="whole" else [x for x in self.ctx["definition"]["phases"] if x["id"]==run["current_phase_id"]],
                             "qualification_reason":"fixture:qualification🙂", "checkpoints":[],"knowledge":None,"knowledge_resources":None})
                value={"run_id":run["id"],"run_revision":run["revision"],"section":p["section"],"data":data}
            else: raise AssertionError("unsupported fake read")
            return FragmentTransport(value,p,page_bytes=4096)(tool,arguments)
        if route=="slice.result.record": return {"error":{"code":"forbidden"}},True
        if p.get("run_revision")!=run["revision"]: return {"error":{"code":"stale_revision"}},True
        if run["status"] in {"completed","escalated","superseded"}: return {"error":{"code":"forbidden"}},True
        if route=="slice.pipeline.delivery.escalate":
            assert run["delivery_mode"]=="whole" and run["current_phase_id"]=="K2"
            run["delivery_mode"]="phasewise"
        elif route=="slice.pipeline.input":
            if p.get("phase_id") != run["current_phase_id"]: return {"error":{"code":"stale_context"}},True
            assert run["status"] in {"active","waiting_input","blocked"}
            run["status"]="active"
        elif route=="slice.pipeline.phase.complete":
            for forbidden in ("consumed_outputs","consumed_inputs","consumed_knowledge"): assert forbidden not in p
            for forbidden in ("skill_reads","resource_reads"): assert forbidden not in p["output"]
            phase=next(x for x in self.ctx["definition"]["phases"] if x["id"]==run["current_phase_id"])
            fixture_constraints(p,phase)
            if p.get("revisit_phase_id"):
                assert p["revisit_phase_id"] in phase["allowed_backward_to"]
                run["current_phase_id"]=p["revisit_phase_id"]
                run["status"]="active"
            elif p["outcome"]=="completed":
                if p["transition"]=="complete":
                    assert run["current_phase_id"]=="K5" and p.get("terminal_result")
                    run["status"]="completed";run["current_phase_id"]=None
                else: run["current_phase_id"]=pe.LIGHTWEIGHT_PHASES[run["current_phase_ordinal"]]
            if not p.get("revisit_phase_id") and p["outcome"] in {"waiting_input","blocked"}: run["status"]=p["outcome"]
            if p.get("terminal_result"):
                result={"id":f"fixture-result-{len(self.history)+1}","outcome":p["outcome"],"provenance":"externally_reported","pipeline_run_id":run["id"],**copy.deepcopy(p["terminal_result"])}
                self.history.append(result);self.ctx["result"]=result
        else: raise AssertionError("unsupported fake command")
        run["revision"]+=1
        run["current_phase_ordinal"]=pe.LIGHTWEIGHT_PHASES.index(run["current_phase_id"])+1 if run["current_phase_id"] else None
        return self.compact("context",self.ctx.get("result")),False


class CurrentContractTests(unittest.TestCase):
    def test_current_and_compatibility_pins_have_distinct_physical_and_semantic_identity(self):
        for version,pin in pe.LIGHTWEIGHT_PINS.items():
            d=definition(version)
            self.assertEqual(d["digest"],pin);self.assertEqual([p["id"] for p in d["phases"]],pe.LIGHTWEIGHT_PHASES)
            self.assertEqual(d["default_mode"],"phasewise");self.assertEqual(d["allowed_modes"],["whole","phasewise"])
        self.assertNotEqual(*pe.LIGHTWEIGHT_PINS.values())
        raw=(DEFINITIONS/f"lightweight-tdd-{pe.LIGHTWEIGHT_VERSION}.json").read_bytes()
        self.assertEqual(hashlib.sha256(raw).hexdigest(),"f3c09714c8040d08d5aff64026cf9dceb5ad1e343033641f27f5629cc0e4e2c5")
        self.assertNotEqual(hashlib.sha256(raw).hexdigest(),pe.LIGHTWEIGHT_PINS[pe.LIGHTWEIGHT_VERSION])
    def test_all_native_probe_pins_match_actual_current_other_pipeline_definitions(self):
        files={"slice.lightweight-tdd-development":f"lightweight-tdd-{pe.LIGHTWEIGHT_VERSION}.json",
               "slice.full-design-to-execution":"full-design-to-execution.json", "slice.debug-root-cause":"debug-root-cause.json",
               "slice.operational-preparation":"operational-preparation.json", "slice.operational-execution":"operational-execution.json",
               "slice.research":"research.json", "slice.deep-brainstorming":"deep-brainstorming.json", "slice.custom-procedure-capture":"procedure-capture.json"}
        for kind,filename in files.items():
            with self.subTest(kind=kind):
                d=json.loads((DEFINITIONS/filename).read_text())
                self.assertEqual((d["version"],d["digest"]),native_slices.PIPELINE_DEFINITIONS[kind])
                self.assertEqual((d["default_mode"],d["allowed_modes"],len(d["phases"])),native_slices.PIPELINE_MODES[kind])
    def test_whole_delivery_assertions_use_retrieved_five_phase_snapshot(self):
        checks=[]
        pe.assert_lightweight_whole_context(context(),lambda name,passed,detail:checks.append((name,passed)))
        native_slices.assert_exact_pipeline_delivery(context(),pe.LIGHTWEIGHT_KIND,lambda name,passed,detail:checks.append((name,passed)))
        self.assertTrue(all(passed for _,passed in checks))
    def test_current_contracts_do_not_declare_retired_artifact_or_validator_carriers(self):
        for phase in definition()["phases"]:
            self.assertFalse(phase.get("required_artifacts",[]));self.assertFalse(phase.get("validator_contracts",[]))
            self.assertTrue(phase["output_constraints"])
    def test_default_and_explicit_begin_are_separate_and_both_hydrate_compact_content(self):
        for explicit in (False,True):
            transport=FakePipeline()
            params=pe.begin_params("scope","slice",1,**({"definition_version":pe.LIGHTWEIGHT_VERSION,"delivery_mode":"whole"} if explicit else {}))
            self.assertEqual("definition_version" in params,explicit)
            begun=native_slices.ok(transport,"command","slice.pipeline.begin",params)["created"]
            self.assertEqual(begun["run"]["delivery_mode"],"whole" if explicit else "phasewise")
            self.assertEqual(len(begun["definition"]["phases"]),5)
            self.assertEqual(len(begun["delivered_phases"]),5 if explicit else 1)
            self.assertEqual(begun["delivery_scope"],"snapshot_reference")
            self.assertIn("retrieved_content",begun)
            self.assertIn("phase_contract",transport.reads)
            self.assertTrue(all(t=="query" for t,a in transport.calls[1:]))
    def test_hydration_rejects_retrieved_identity_or_reserved_details_changes(self):
        for view in ("snapshot","details"):
            fake=FakePipeline()
            def modified(tool,args):
                if tool=="query" and args["params"].get("view")==view:
                    p=args["params"]
                    if view=="snapshot":
                        altered=copy.deepcopy(fake.ctx["definition"]);altered["version"]="changed"
                        return {"run_id":p["run_id"],"definition_digest":p["definition_digest"],"definition":altered},False
                    return {"run_id":p["run_id"],"run_revision":p["run_revision"],"section":p["section"],"data":{"run":{"id":"changed"}}},False
                return fake(tool,args)
            with self.assertRaises(AssertionError):common.hydrate_pipeline_payload(modified,fake.compact())
    def test_all_five_builders_satisfy_source_constraints_without_agent_receipt_keys(self):
        for version in pe.LIGHTWEIGHT_PINS:
            for phase in definition(version)["phases"]:
                with self.subTest(version=version,phase=phase["id"]):
                    params=pe.completion_params(context(phase["id"],version=version),transition="complete" if phase["id"]=="K5" else "continue")
                    fixture_constraints(params,phase)
                    self.assertFalse(set(params)&{"consumed_outputs","consumed_inputs","consumed_knowledge"})
                    self.assertFalse(set(params["output"])&{"skill_reads","resource_reads","artifacts","validator_receipts"})
                    self.assertIn("Structural fixture only",params["output"]["body"])
    def test_self_review_and_local_only_fixture_claims(self):
        output=pe.completion_params(context("K3"))["output"]
        self.assertEqual(output["fields"]["review_mode"],"self")
        self.assertNotIn("reviewer_context",output)
        self.assertFalse(definition()["phases"][2]["fresh_reviewer_input"])
        final=pe.completion_params(context("K5"),transition="complete")["output"]
        self.assertEqual(final["fields"]["truth_level"],"local_verified")
        self.assertEqual(final["fields"]["promotion"],"no_promotion")
        self.assertEqual(final["fields"]["handoff"],"none")
        self.assertIn("no commands executed",final["body"])
    def test_fresh_review_phase_requires_explicit_context_without_fabricated_provenance(self):
        d=json.loads((DEFINITIONS/"full-design-to-execution.json").read_text())
        phase=next(p for p in d["phases"] if p.get("fresh_reviewer_input") is True)
        ctx=context();ctx["definition"]=d
        ctx["run"].update(definition_kind=d["kind"],definition_version=d["version"],definition_digest=d["digest"],current_phase_id=phase["id"])
        with self.assertRaisesRegex(AssertionError,"explicit reviewer_context required"):
            pe.completion_params(ctx)
        explicit={"reviewer_identity":"caller-supplied-actor", "reviewer_context_id":"caller-supplied-session", "producer_context_ids":["caller-supplied-producer"], "fresh_input":False}
        result=pe.completion_params(ctx,reviewer_context=explicit)
        self.assertEqual(json.dumps(result["output"]["reviewer_context"]),json.dumps(explicit))
        self.assertFalse(result["output"]["reviewer_context"]["fresh_input"])
        self.assertEqual(explicit["producer_context_ids"],["caller-supplied-producer"])
    def test_requested_independent_mode_is_only_explicit_request_construction(self):
        with self.assertRaisesRegex(AssertionError,"explicit reviewer_context required"):
            pe.completion_params(context("K3"),review_mode="independent")
        explicit={"reviewer_identity":"caller-supplied-actor", "reviewer_context_id":"caller-supplied-session", "producer_context_ids":["caller-supplied-producer"], "fresh_input":True}
        result=pe.completion_params(context("K3"),review_mode="independent",reviewer_context=explicit)
        self.assertEqual(json.dumps(result["output"]["reviewer_context"]),json.dumps(explicit))
        self.assertEqual(result["output"]["fields"]["reviewer"],explicit["reviewer_identity"])
        self.assertIn("require backend checks",result["output"]["fields"]["findings"])
        self.assertEqual(result["output"]["fields"]["review_mode"],"independent")
        self.assertNotIn("reviewer_context",pe.completion_params(context("K3"))["output"])
    def test_reviewer_context_rejects_unsupported_fields_or_missing_identity(self):
        with self.assertRaisesRegex(AssertionError,"unsupported contract fields"):
            pe.completion_params(context("K3"),review_mode="independent",reviewer_context={"native_session":"fabricated"})
        with self.assertRaisesRegex(AssertionError,"explicit reviewer_identity required"):
            pe.completion_params(context("K3"),review_mode="independent",reviewer_context={})
    def test_receipt_same_target_and_requested_unique_scopes(self):
        redgreen=pe.completion_params(context("K4"))["output"]["fields"]
        red,green=json.loads(redgreen["red_receipt"]),json.loads(redgreen["green_receipt"])
        self.assertEqual(red["target"],green["target"]);self.assertEqual(red["target"],redgreen["target_binding"])
        self.assertNotEqual(red["exit_code"],0);self.assertEqual(green["exit_code"],0)
        final=pe.completion_params(context("K5"),transition="complete")["output"]["fields"]
        for field,scope in (("focused_proof","focused"),("affected_proof","affected")):
            receipt=json.loads(final[field]);self.assertEqual(receipt["scopes"],[scope]);self.assertTrue(receipt["fresh"]);self.assertFalse(receipt["skipped"])
    def test_retired_or_unknown_lightweight_context_is_never_executable(self):
        for version in ("0.6.0-native.engineering.2","unknown","0.7.0"):
            ctx=context();ctx["run"]["definition_version"]=version
            with self.assertRaises(AssertionError): pe.completion_params(ctx)
    def test_legacy_other_pipeline_keeps_exact_consumption_and_receipts(self):
        ctx=context();ctx["run"]["definition_kind"]="slice.debug-root-cause";ctx["run"]["definition_version"]="0.4.0-native.skills.2"
        ctx["definition"]["phases"][0]["id"]="legacy-test-phase";ctx["run"]["current_phase_id"]="legacy-test-phase"
        ctx["consumed_outputs"]=[{"phase_id":"previous","output_revision":1,"digest":"source-pin"}]
        ctx["consumed_inputs"]=[];ctx["consumed_knowledge"]={"manifest_id":"manifest","digest":"knowledge-pin"}
        params=pe.completion_params(ctx)
        self.assertEqual(params["consumed_outputs"],ctx["consumed_outputs"])
        self.assertEqual(params["consumed_knowledge"],ctx["consumed_knowledge"])
        self.assertIn("skill_reads",params["output"]);self.assertIn("resource_reads",params["output"])
    def test_fixture_lifecycle_delivery_rework_resume_block_history_and_new_terminal_carrier(self):
        fake=FakePipeline();ctx=context()
        ctx=pe.ok(fake,"command","slice.pipeline.phase.complete",pe.completion_params(ctx))["context"]
        ctx=pe.ok(fake,"command","slice.pipeline.delivery.escalate",{"request_id":"fixture","run_id":ctx["run"]["id"],"run_revision":ctx["run"]["revision"],"phase_id":"K2","reason":"fixture delivery switch"})["context"]
        self.assertEqual(ctx["run"]["delivery_mode"],"phasewise");self.assertEqual(len(ctx["delivered_phases"]),1)
        ctx=pe.ok(fake,"command","slice.pipeline.phase.complete",pe.completion_params(ctx,outcome_name="waiting_input"))["context"]
        ctx=pe.ok(fake,"command","slice.pipeline.input",{"run_revision":ctx["run"]["revision"],"phase_id":"K2","input":"fixture resume"})["context"]
        ctx=pe.ok(fake,"command","slice.pipeline.phase.complete",pe.completion_params(ctx))["context"]
        ctx=pe.rework_and_resume(fake,ctx,"K2")
        for _ in range(3): ctx=pe.ok(fake,"command","slice.pipeline.phase.complete",pe.completion_params(ctx))["context"]
        self.assertEqual(ctx["run"]["current_phase_id"],"K5")
        ctx=pe.rework_and_resume(fake,ctx,"K4")
        ctx=pe.ok(fake,"command","slice.pipeline.phase.complete",pe.completion_params(ctx))["context"]
        blocked={"summary":"fixture blocked","evidence":[{"kind":"fixture_observation"}],"scope_impact":"fixture","remaining_work":"fixture resume"}
        ctx=pe.ok(fake,"command","slice.pipeline.phase.complete",pe.completion_params(ctx,outcome_name="blocked",transition="block",terminal_result=blocked,publish_blocked_result=True))["context"]
        first=copy.deepcopy(fake.history[0]);old_revision=ctx["run"]["revision"]
        ctx=pe.ok(fake,"command","slice.pipeline.input",{"run_revision":old_revision,"phase_id":"K5","input":"fixture resumed"})["context"]
        completed={"summary":"new current fixture completion","evidence":[{"kind":"fixture_observation","reference":"fixture:new-revision"}],"scope_impact":"fixture","remaining_work":"actual QA unperformed"}
        params=pe.completion_params(ctx,transition="complete",terminal_result=completed)
        self.assertGreater(params["run_revision"],old_revision)
        final=pe.ok(fake,"command","slice.pipeline.phase.complete",params)
        self.assertEqual(final["result"]["summary"],completed["summary"])
        self.assertEqual(fake.history[0],first);self.assertEqual(len(fake.history),2)
        duplicate,failed=fake("command",{"route":"slice.pipeline.phase.complete","params":{**params,"run_revision":fake.ctx["run"]["revision"]}})
        self.assertTrue(failed);self.assertEqual(duplicate["error"]["code"],"forbidden")
        bypass,failed=fake("command",{"route":"slice.result.record","params":{}})
        self.assertTrue(failed);self.assertEqual(bypass["error"]["code"],"forbidden")
    def test_undeclared_rework_is_rejected_before_transport(self):
        with self.assertRaises(AssertionError): pe.completion_params(context("K3"),outcome_name="waiting_input",revisit_phase_id="K1")
    def test_static_driver_inventory_keeps_planning_and_guard_scenarios(self):
        source=Path(native_slices.__file__).read_text()
        for marker in ("Decision-to-Lightweight", "slice.result.record", "publish_blocked_result=True", "rework_and_resume", "completed_result", "scope.candidates", "slice.candidates.refresh"):
            self.assertIn(marker,source)
        self.assertEqual(native_slices.PIPELINE_MODES[pe.LIGHTWEIGHT_KIND][2],5)
        self.assertEqual(native_slices.PIPELINE_DEFINITIONS[pe.LIGHTWEIGHT_KIND],(pe.LIGHTWEIGHT_VERSION,pe.LIGHTWEIGHT_PINS[pe.LIGHTWEIGHT_VERSION]))
        self.assertNotIn('"0.6.0-native.engineering.2"',source)
        self.assertNotIn('127',source)


class PinnedReaderTests(unittest.TestCase):
    def setUp(self):
        self.params={"run_id":"run","view":"details","run_revision":7,"section":"all"}
        self.value={"run_id":"run","run_revision":7,"section":"all","data":{"input":"🙂中\\\""*25}}
    def read(self,transport): return common.read_pipeline_json(transport,"query","slice.pipeline.context",self.params)
    def test_original_utf8_hash_unicode_offsets_and_collection_cursor_only_after_eof(self):
        cursor={"tool":"query","arguments":{"route":"collection.next","params":{"cursor":"after-eof"}}}
        transport=FragmentTransport(self.value,self.params,page_bytes=41,terminal_actions=[cursor])
        result=self.read(transport)
        self.assertEqual(result["data"],self.value["data"]);self.assertEqual(result["actions"],[cursor])
        self.assertNotEqual(hashlib.sha256(json.dumps(self.value).encode()).hexdigest(),transport.digest)
        self.assertGreater(len(transport.calls),2)
        for tool,args in transport.calls[1:]:
            self.assertEqual(args["params"]["representation_digest"],transport.digest)
            self.assertEqual(args["params"]["limit_bytes"],4096)
            self.assertEqual(args["params"]["run_revision"],7)
    def test_small_original_shape_needs_no_byte_pagination(self):
        original={**self.value,"actions":[]}
        calls=[]
        def transport(t,a):calls.append((t,a));return copy.deepcopy(original),False
        self.assertEqual(self.read(transport),original);self.assertEqual(len(calls),1)
    def test_pin_revision_and_digest_changes_are_rejected(self):
        mutations=[lambda p,n:p["source"].update(run_revision=8) if n>1 else None,
                   lambda p,n:p.update(representation_digest="0"*64) if n>1 else None,
                   lambda p,n:p.update(total_bytes=p["total_bytes"]+1) if n>1 else None]
        for mutation in mutations:
            with self.subTest(mutation=mutation):
                with self.assertRaises(AssertionError): self.read(FragmentTransport(self.value,self.params,mutate=mutation))
    def test_bad_offset_nonprogress_oversize_length_and_missing_eof_are_rejected(self):
        mutations=[lambda p,n:p.update(offset_bytes=1),lambda p,n:p.update(next_offset_bytes=0),
                   lambda p,n:p.update(returned_bytes=p["returned_bytes"]+1),lambda p,n:p.pop("next_offset_bytes"),
                   lambda p,n:p.update(returned_bytes=4097,text="x"*4097,total_bytes=5000),
                   lambda p,n:p.update(next_offset_bytes=None)]
        for mutation in mutations:
            with self.subTest(mutation=mutation):
                with self.assertRaises(AssertionError): self.read(FragmentTransport(self.value,self.params,mutate=mutation))
    def test_wrong_continuation_pin_and_early_collection_action_are_rejected(self):
        def changed(p,n): p["actions"][0]["arguments"]["params"]["run_revision"]=8
        def collection(p,n): p["actions"].append({"tool":"query","arguments":{"route":"collection.next","params":{}}})
        for mutation in (changed,collection):
            with self.assertRaises(AssertionError): self.read(FragmentTransport(self.value,self.params,mutate=mutation))
    def test_full_original_digest_checked_before_json_parsing(self):
        transport=FragmentTransport(self.value,self.params)
        transport.original=transport.original.replace(b'"data"',b'"date"')
        with self.assertRaisesRegex(AssertionError,"digest mismatch"): self.read(transport)
    def test_small_shape_wrong_source_pin_rejected(self):
        with self.assertRaises(AssertionError): self.read(lambda t,a:({**self.value,"run_revision":8},False))
    def test_first_fragment_missing_source_pin_and_empty_nonprogress_are_rejected(self):
        def source_changed(p,n): p["source"].pop("run_revision")
        def empty(p,n): p.update(text="",returned_bytes=0,next_offset_bytes=p["offset_bytes"])
        for mutate in (source_changed,empty):
            with self.assertRaises(AssertionError): self.read(FragmentTransport(self.value,self.params,mutate=mutate))
    def test_phase_contract_pin_is_checked_in_small_and_fragment_shapes(self):
        params={"run_id":"run","view":"phase_contract","definition_digest":"pin","phase_id":"K3"}
        value={"run_id":"run","definition_digest":"pin","phase":{"id":"K3","instructions":[{"body":"🙂"*20}]}}
        self.assertEqual(common.read_pipeline_json(FragmentTransport(value,params),"query","slice.pipeline.context",params)["phase"],value["phase"])
        wrong={**value,"phase":{"id":"K2"}}
        with self.assertRaises(AssertionError): common.read_pipeline_json(lambda t,a:(wrong,False),"query","slice.pipeline.context",params)
    def test_explicit_byte_read_and_initial_representation_pin_are_enforced(self):
        params={**self.params,"offset_bytes":0,"representation_digest":"0"*64}
        with self.assertRaisesRegex(AssertionError,"initial representation pin changed"):
            common.read_pipeline_json(FragmentTransport(self.value,params),"query","slice.pipeline.context",params)
        with self.assertRaisesRegex(AssertionError,"did not return a fragment"):
            common.read_pipeline_json(lambda t,a:(self.value,False),"query","slice.pipeline.context",{**self.params,"limit_bytes":4096})
    def test_fragment_failure_never_returns_partial_content(self):
        transport=FragmentTransport(self.value,self.params)
        def failing(t,a):
            if a["params"].get("offset_bytes",0)>0:return {"error":{"code":"STALE_REVISION"}},True
            return transport(t,a)
        with self.assertRaises(AssertionError): self.read(failing)


if __name__=="__main__":unittest.main()
