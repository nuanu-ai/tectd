"""Real synthetic child exercises ordinary private failure finalization, never a model."""
import hashlib,json,os,stat,subprocess,sys,tempfile,unittest
from pathlib import Path
from unittest.mock import patch
from types import SimpleNamespace
from scripts import bounded_caller_route_launcher as launcher
from scripts.caller_host_routing import CallerRoutingRequest,CurrentHostSelection,_json
from scripts.private_owned_appserver_profile import PrivateOwnedLaunchProfile
from scripts.private_s05_evidence import PrivateS05EvidenceRejected
from scripts.codex_app_server_rpc import OwnedAppServerRpc

SERVER=r'''
import json,sys,pathlib
home=pathlib.Path(sys.argv[1])
def emit(value):print(json.dumps(value),flush=True)
for line in sys.stdin:
 r=json.loads(line);m=r['method'];p=r.get('params',{})
 if m=='initialized':continue
 if m=='initialize':result={'fixture':True}
 elif m=='thread/start':
  result={'thread':{'id':'owned'}}
  emit({'method':'thread/started','params':result})
 elif m=='private/s05/session.read':
  result={'threadId':'owned','capturedSession':{'model':'gpt-6-luna','modelProvider':'openai','effort':'xhigh','approvalPolicy':'never','sandboxPolicy':{'type':'read-only'},'cwd':str(home),'allowedTools':[],'ephemeral':False,'requestMaxRetries':0,'streamMaxRetries':0,'source':'capturedSessionConfigurationAndToolPolicy'},'enforcedDispatch':{'allowProviderModelFallback':False,'source':'ownedThreadStartGuard'},'source':'ownedThreadSession'}
 elif m=='turn/start':
  nested=home/'sessions'/'nested';nested.mkdir(parents=True)
  path=nested/'rollout-fixture.jsonl';path.write_text('{}\n');path.chmod(0o644)
  result={'turn':{'id':'turn'}}
  emit({'method':'private/s05/outboundObserved','params':{'threadId':'owned','turnId':'turn','model':'gpt-6-luna','effort':'xhigh','toolNames':None,'toolsSha256':None,'requestSha256':'a'*64,'source':'serializedResponsesRequest'}})
  emit({'method':'turn/completed','params':{'threadId':'owned','turn':{'id':'turn','status':'completed','error':None}}})
  sys.stderr.write('ordinary failure fixture stderr\nBearer fixture-secret\n');sys.stderr.flush()
 else:raise RuntimeError('unexpected fixture method')
 emit({'id':r['id'],'result':result})
emit({'method':'fixture/lateCleanupNotice','params':{'threadId':'owned'}})
sys.exit(17)
'''

class OrdinaryFailureTests(unittest.TestCase):
 def test_real_private_child_rejection_finalizes_actual_session_trace_exit_and_unknowns(self):
  with tempfile.TemporaryDirectory(dir='/private/tmp') as temp:
   root=Path(temp);root.chmod(0o700);home=root/'home';home.mkdir(mode=0o700);ledger=root/'ledger';ledger.mkdir(mode=0o700)
   uuid=lambda n:'00000000-0000-0000-0000-'+f'{n:012d}'
   request=CallerRoutingRequest('fake-prepared',uuid(1),uuid(2),uuid(3),1,'a'*64,'b'*64,'luna','fake-only-invocation')
   selected={'route_id':'luna','provider':'openai','model':'gpt-6-luna','effort':'xhigh'}
   material={'selected_route':selected,'requested_route':selected,'recommended_route':selected,'preparation':{'work':{'available_latency_ms':{'Known':{'value':60000}}}}}
   canonical=_json(material);snapshot=CurrentHostSelection(canonical,hashlib.sha256(canonical.encode()).hexdigest())
   class Source:
    def __init__(self,*args):self.calls=0
    def resolve_current(self,*args,**kwargs):self.calls+=1;return snapshot
   source=Source();argv=(sys.executable,'-c',SERVER,str(home),'--private-runtime-home',str(home))
   profile=PrivateOwnedLaunchProfile(argv,'d'*64,0,0,(('RUST_MIN_STACK','8388608'),))
   parent_mask=subprocess.check_output(['sh','-c','umask']).strip()
   with patch.object(launcher,'_ExecutorSource',return_value=source),patch.object(launcher,'_validate_material',return_value=material),patch.object(launcher,'_host_profile',return_value=profile):
    with self.assertRaises(PrivateS05EvidenceRejected) as rejected:
     launcher._execute_owned(host_context=SimpleNamespace(),request=request,prompt='synthetic prompt',cwd=str(home),ledger_dir=ledger,executable=sys.executable)
   self.assertEqual(source.calls,3);self.assertEqual(subprocess.check_output(['sh','-c','umask']).strip(),parent_mask)
   failure=json.loads(Path(rejected.exception._private_failure_result).read_text());receipt=json.loads(Path(rejected.exception._private_failure_receipt).read_text())
   self.assertEqual(failure['failure_reason'],'rollout path is not private owned regular storage')
   self.assertEqual(failure['failure_stage'],'pre_history_storage_snapshot')
   self.assertEqual(failure['partial_observation']['captured_session']['allowedTools'],[])
   diagnostics=failure['child_diagnostics'];self.assertEqual(diagnostics['child_exit_code'],17);self.assertTrue(diagnostics['stdout_eof_observed']);self.assertTrue(diagnostics['reader_threads_stopped'])
   self.assertIn('ordinary failure fixture stderr',diagnostics['stderr_redacted']);self.assertNotIn('fixture-secret',diagnostics['stderr_redacted'])
   self.assertEqual(diagnostics['stderr_raw_sha256'],hashlib.sha256(b'ordinary failure fixture stderr\nBearer fixture-secret\n').hexdigest())
   frames=diagnostics['private_wire_frames'];self.assertTrue(any(n['direction']=='server_frame' and 'result' in n['frame'] and n['frame']['result'].get('threadId')=='owned' for n in frames))
   self.assertIn('fixture/lateCleanupNotice',[n['method'] for n in diagnostics['stdout_notification_trace']])
   self.assertFalse(failure['clean_seal_acceptance']);self.assertFalse(diagnostics['clean_notification_seal']);self.assertIsNone(failure['observed_actual']);self.assertEqual(failure['model_observations'],[]);self.assertIsNone(failure['outbound_observations'][0]['toolNames'])
   self.assertEqual(receipt['result_digest'],hashlib.sha256(_json(failure).encode()).hexdigest());self.assertFalse(receipt['automatic_retry'])
   self.assertEqual(stat.S_IMODE((home/'sessions'/'nested').stat().st_mode),0o700)
   original_result=Path(rejected.exception._private_failure_result).read_bytes()
   with patch.object(launcher,'_ExecutorSource') as fake:
    with self.assertRaises(launcher.CallerRoutingRejected):launcher._execute_owned(host_context=SimpleNamespace(),request=request,prompt='synthetic prompt',cwd=str(home),ledger_dir=ledger,executable=sys.executable)
    fake.assert_not_called()
   self.assertEqual(Path(rejected.exception._private_failure_result).read_bytes(),original_result)
 def test_private_child_new_nested_storage_is_0700_0600_without_parent_umask_change(self):
  with tempfile.TemporaryDirectory(dir='/private/tmp') as temp:
   root=Path(temp);mask=subprocess.check_output(['sh','-c','umask']).strip()
   code='import pathlib,json,sys; p=pathlib.Path("new/nested"); p.mkdir(parents=True); (p/"file").write_text("fixture"); r=json.loads(sys.stdin.readline()); print(json.dumps({"id":r["id"],"result":{}}),flush=True); sys.stdin.read()'
   with OwnedAppServerRpc([sys.executable,'-c',code],str(root),private_stack_environment={'RUST_MIN_STACK':'8388608'}) as rpc:
    rpc.request('initialize',{},timeout=5);rpc.seal_notifications(timeout=5)
   self.assertEqual(stat.S_IMODE((root/'new').stat().st_mode),0o700);self.assertEqual(stat.S_IMODE((root/'new/nested').stat().st_mode),0o700);self.assertEqual(stat.S_IMODE((root/'new/nested/file').stat().st_mode),0o600)
   self.assertEqual(subprocess.check_output(['sh','-c','umask']).strip(),mask)
if __name__=='__main__':unittest.main()
