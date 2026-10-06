import hashlib,json,tempfile,unittest
from pathlib import Path
from scripts.test_private_s05_adapter import FakeRpc
from scripts.private_s05_adapter import observe_private,CHECKER,CASE_NONCE
from scripts.private_s05_seal import PrivateSealLedger
from scripts.private_s05_acceptance import evaluate
from scripts.caller_host_routing import _json
from scripts.codex_app_server_observer import _sha
from scripts.private_s05_evidence import PrivateS05EvidenceRejected
class AcceptanceTests(unittest.TestCase):
    def setUp(self):
        self.temp=tempfile.TemporaryDirectory(dir='/private/tmp');self.home=Path(self.temp.name);self.home.chmod(0o700);self.path=self.home/'ledger';self.path.mkdir(mode=0o700)
        ns={'__name__':'offline_test'};exec(CHECKER.read_bytes(),ns);prompt=ns['build_prompt'](CASE_NONCE);output=json.dumps(ns['expected_result'](CASE_NONCE))
        intent={'invocation_key':'offline-test','source_binding_json':'{}','source_binding_digest':_sha('{}'),'launch_profile_digest':'b'*64,'prompt_sha256':_sha(prompt),'cwd':str(self.home),'declared_available_latency_ms':1000}
        intent_digest=_sha(_json(intent));seal=PrivateSealLedger(self.path);seal.reserve(invocation_key='offline-test',intent_digest=intent_digest,prompt_sha256=_sha(prompt),source_binding_sha256=_sha('{}'),profile_sha256='b'*64,intent=intent)
        rpc=FakeRpc(self.home,prompt,output)
        if self._testMethodName.startswith('test_object_'):rpc.model['source']='remoteResponseObjectModel'
        if 'agreeing_header' in self._testMethodName:rpc.additional_models=[{**rpc.model,'source':'sseResponseHeaders'}]
        result=observe_private(rpc,prompt=prompt,cwd=str(self.home),invocation_key='offline-test',seal_ledger=seal,intent_digest=intent_digest)
        receipt={'schema':'jev.s05.private-receipt/1','status':'completed_observed_private_route','invocation_key':'offline-test','intent':intent,'result':result,'intent_digest':intent_digest,'result_digest':_sha(_json(result)),'observed_actual':{'provider':'openai','model':'gpt-6-luna','effort':'UNKNOWN'}};seal.record('04-receipt',receipt)
    def tearDown(self):self.temp.cleanup()
    def test_independent_raw_seal_evaluator_retains_unknown_server_effort(self):
        result=evaluate(self.path,'offline-test');self.assertEqual(result['status'],'passed_retained_checks');self.assertEqual(result['server_reported_effort'],'UNKNOWN');self.assertIn('not_new_authentication',result['provenance'])
    def test_raw_rollout_tamper_fails_independent_digest(self):
        path=self.path/(_sha('offline-test')+'.02-rollout.jsonl');path.write_bytes(path.read_bytes()+b'\n')
        with self.assertRaises(PrivateS05EvidenceRejected):evaluate(self.path,'offline-test')
    def test_object_only_independent_acceptance_reports_provider_declared_id_and_unknown_backend(self):
        result=evaluate(self.path,'offline-test');self.assertEqual(result['provider_model_metadata_sources'],['remoteResponseObjectModel']);self.assertEqual(result['model_evidence_level'],'provider_declared_used_model_id');self.assertEqual(result['internal_backend_model'],'UNKNOWN');self.assertEqual(result['server_reported_effort'],'UNKNOWN')
    def test_object_with_agreeing_header_independent_acceptance_preserves_both_sources(self):
        result=evaluate(self.path,'offline-test');self.assertEqual(result['provider_model_metadata_sources'],['remoteResponseObjectModel','sseResponseHeaders'])
    def test_object_forged_backend_attestation_or_effort_cannot_replace_unknown(self):
        key=_sha('offline-test');receipt_path=self.path/(key+'.04-receipt.json');result_path=self.path/(key+'.03-result.json');original=json.loads(receipt_path.read_text())
        for field,value in [('internal_backend_model','gpt-6-luna-internal'),('model_evidence_level','cryptographic_backend_attestation'),('server_reported_effort','xhigh')]:
            receipt=json.loads(json.dumps(original));receipt['result'][field]=value;receipt['result_digest']=_sha(_json(receipt['result']));result_path.write_text(_json(receipt['result']));receipt_path.write_text(_json(receipt))
            with self.assertRaises(PrivateS05EvidenceRejected):evaluate(self.path,'offline-test')
if __name__=='__main__':unittest.main()
