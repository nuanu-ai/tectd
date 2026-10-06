import unittest,sys,tempfile,pathlib,json
from scripts.private_s05_readiness import inspect_readiness,execute_probe
from scripts.private_s05_evidence import PrivateS05EvidenceRejected
class Rpc:
    def __init__(self):self.calls=[];self.efforts=[{'reasoningEffort':'xhigh'}];self.trace=[]
    def initialize(self,*args,**kwargs):self.calls.append('initialize')
    def request(self,method,params,timeout):
        self.calls.append(method)
        if method=='model/list':return {'data':[{'model':'gpt-6-luna','supportedReasoningEfforts':self.efforts}],'nextCursor':None}
        if method=='thread/start':return {'thread':{'id':'owned'}}
        if method=='private/s05/session.read':return {'threadId':'owned','capturedSession':{'model':'gpt-6-luna','modelProvider':'openai','effort':'xhigh','approvalPolicy':'never','sandboxPolicy':{'type':'read-only','network_access':False},'cwd':'/private/owned','allowedTools':[],'ephemeral':False,'requestMaxRetries':0,'streamMaxRetries':0,'source':'capturedSessionConfigurationAndToolPolicy'},'enforcedDispatch':{'allowProviderModelFallback':False,'source':'ownedThreadStartGuard'},'source':'ownedThreadSession'}
        raise AssertionError('forbidden request')
    def seal_notifications(self,timeout):return self.trace
class ReadinessTests(unittest.TestCase):
    def test_no_turn_path_has_only_metadata_and_captured_policy(self):
        rpc=Rpc();r=inspect_readiness(rpc,'/private/owned');self.assertEqual(rpc.calls,['initialize','model/list','thread/start','private/s05/session.read']);self.assertFalse(r['inference_turn_sent']);self.assertEqual(r['provider_reported_serving_model'],'UNKNOWN');self.assertEqual(r['final_serialized_tools'],'UNKNOWN')
    def test_missing_effort_fails_before_thread(self):
        rpc=Rpc();rpc.efforts=[]
        with self.assertRaises(PrivateS05EvidenceRejected):inspect_readiness(rpc,'/private/owned')
        self.assertEqual(rpc.calls,['initialize','model/list'])
    def test_real_subprocess_early_eof_retains_stderr_exit_and_closes_child(self):
        with tempfile.TemporaryDirectory(dir='/private/tmp') as d:
            result,error=execute_probe([sys.executable,'-c','import sys; sys.stderr.write("bounded EOF fixture\\n"); sys.stderr.flush(); sys.exit(23)'],d,private_stack_environment={'RUST_MIN_STACK':'8388608'})
        self.assertIsNotNone(error);self.assertEqual(result['child_diagnostics']['child_exit_code'],23);self.assertIn('bounded EOF fixture',result['child_diagnostics']['stderr_redacted']);self.assertFalse(result['inference_turn_sent']);self.assertEqual(result['status'],'failed_closed_no_retry')
    def test_actual_disabled_startup_notice_readiness_retains_full_envelope(self):
        rpc=Rpc();notice=json.loads((pathlib.Path(__file__).parent/'fixtures/private_s05_disabled_startup_notice.json').read_text());rpc.trace=[notice,{'method':'thread/started','params':{'thread':{'id':'owned'}}}]
        result=inspect_readiness(rpc,'/private/owned');self.assertEqual(result['notification_trace'][0],notice)
    def test_late_disabled_startup_notice_readiness_denied(self):
        rpc=Rpc();notice=json.loads((pathlib.Path(__file__).parent/'fixtures/private_s05_disabled_startup_notice.json').read_text());rpc.trace=[{'method':'thread/started','params':{'thread':{'id':'owned'}}},notice]
        with self.assertRaises(PrivateS05EvidenceRejected):inspect_readiness(rpc,'/private/owned')
    def test_actual04_configuration_notices_exact_source_and_runtime_path(self):
        from scripts.private_s05_adapter import expected_skill_warning
        notices=json.loads((pathlib.Path(__file__).parent/'fixtures/private_s05_configuration_notices04.json').read_text())
        actual_home='/private/tmp/jev-s05-readiness04-20261006-pl3oyu8v'
        self.assertEqual(notices[1]['params']['message'],expected_skill_warning(actual_home))
        rpc=Rpc();notices[1]['params']={'threadId':'owned','message':expected_skill_warning('/private/owned')}
        rpc.trace=[{'method':'thread/started','params':{'thread':{'id':'owned'}}}]+notices
        result=inspect_readiness(rpc,'/private/owned');self.assertEqual(result['notification_trace'],rpc.trace)
        rpc.trace.append(notices[1])
        with self.assertRaises(PrivateS05EvidenceRejected):inspect_readiness(rpc,'/private/owned')
    def test_outbound_notice_fails(self):
        rpc=Rpc();rpc.trace=[{'method':'private/s05/outboundObserved','params':{}}]
        with self.assertRaises(PrivateS05EvidenceRejected):inspect_readiness(rpc,'/private/owned')
    def test_failed_readiness_retains_popped_response_and_full_stderr_stream(self):
        import hashlib
        program = """import sys,json
r=json.loads(sys.stdin.readline())
print(json.dumps({'id':r['id'],'result':{}}),flush=True)
sys.stdin.readline()
r=json.loads(sys.stdin.readline())
print(json.dumps({'id':r['id'],'result':{'data':[],'nextCursor':None}}),flush=True)
sys.stderr.write('z'*20000);sys.stderr.flush()
sys.stdin.read();sys.exit(19)
"""
        with tempfile.TemporaryDirectory(dir='/private/tmp') as d:
            result,error=execute_probe([sys.executable,'-c',program],d,private_stack_environment={'RUST_MIN_STACK':'8388608'})
        self.assertIsNotNone(error)
        retained=result['private_rpc_diagnostics']
        self.assertEqual(retained['child_exit_code'],19)
        self.assertTrue(retained['stdout_eof_observed'])
        self.assertTrue(retained['reader_threads_stopped'])
        self.assertFalse(retained['clean_notification_seal'])
        self.assertEqual(retained['stderr_raw_bytes'],20000)
        self.assertEqual(retained['stderr_raw_sha256'],hashlib.sha256(b'z'*20000).hexdigest())
        self.assertLess(retained['stderr_retained_tail_bytes'],20000)
        frames=retained['private_wire_frames']
        self.assertTrue(any(f['direction']=='server_frame' and f['frame'].get('result')=={'data':[],'nextCursor':None} for f in frames))
        methods=[f['frame'].get('method') for f in frames if f['direction']=='client_request']
        self.assertNotIn('turn/start',methods)
        for field in ('provider_reported_serving_model','serialized_request_effort','server_reported_effort','final_serialized_tools','usage'):
            self.assertEqual(result[field],'UNKNOWN')
        self.assertFalse(result['notification_capture_complete'])
if __name__=='__main__':unittest.main()
