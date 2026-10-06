import copy,hashlib,json,tempfile,unittest
from pathlib import Path
from scripts.private_s05_adapter import observe_private,semantic_check,CASE_NONCE,CHECKER
from scripts.private_s05_evidence import PrivateS05EvidenceRejected
from scripts.private_s05_seal import PrivateSealLedger

class FakeRpc:
    def __init__(self,home,prompt,output):
        self.home=home;self.prompt=prompt;self.output=output;self.calls=[];self.sealed=False;self.additional_models=[]
        self.model={'threadId':'thread','turnId':'turn','responseId':'resp','model':'gpt-6-luna','source':'sseResponseHeaders'}
        self.out={'threadId':'thread','turnId':'turn','model':'gpt-6-luna','effort':'xhigh','toolNames':[],'toolsSha256':hashlib.sha256(b'[]').hexdigest(),'requestSha256':'a'*64,'source':'serializedResponsesRequest'}
        self.completion={'method':'turn/completed','params':{'threadId':'thread','turn':{'id':'turn','status':'completed','error':None}}}
        self.captured={'model':'gpt-6-luna','modelProvider':'openai','effort':'xhigh','approvalPolicy':'never','sandboxPolicy':{'type':'read-only'},'cwd':str(home),'allowedTools':[],'ephemeral':False,'requestMaxRetries':0,'streamMaxRetries':0,'source':'capturedSessionConfigurationAndToolPolicy'}
    def request(self,method,params,timeout=None):
        assert not self.sealed;self.calls.append((method,copy.deepcopy(params)))
        if method=='thread/start':return {'thread':{'id':'thread'}}
        if method=='private/s05/session.read':return {'threadId':'thread','capturedSession':self.captured,'enforcedDispatch':{'allowProviderModelFallback':False,'source':'ownedThreadStartGuard'},'source':'ownedThreadSession'}
        if method=='turn/start':self.write();return {'turn':{'id':'turn'}}
        if method=='private/s05/history.read':
            path=self.home/'rollout.jsonl'
            return {'threadId':'thread','turnId':'turn','rolloutPath':str(path),'rolloutSha256':hashlib.sha256(path.read_bytes()).hexdigest(),'prompt':self.prompt,'output':self.output,'status':'completed','modelObservations':[self.model,*self.additional_models],'outboundObservations':[self.out],'source':'committedOwnedRollout'}
        raise AssertionError(method)
    def write(self):
        events=[('session_meta',{'id':'thread'}),('event_msg',{'type':'thread_settings_applied','thread_id':'thread','thread_settings':{'model':'gpt-6-luna','model_provider_id':'openai','reasoning_effort':'xhigh','approval_policy':'never'}}),('response_item',{'type':'agent_message','author':'assistant','recipient':'all','content':[{'type':'input_text','text':self.output}]}),('turn_context',{'turn_id':'turn'}),('event_msg',{'type':'task_started','turn_id':'turn'}),('event_msg',{'type':'user_message','message':self.prompt}),('event_msg',{'type':'private_s05_model_observed','turn_id':'turn','response_id':'resp','model':self.model['model'],'source':self.model['source']}),('event_msg',{'type':'private_s05_outbound_observed','turn_id':'turn','model':self.out['model'],'effort':self.out['effort'],'tool_names':self.out['toolNames'],'tools_sha256':self.out['toolsSha256'],'request_sha256':self.out['requestSha256'],'source':self.out['source']}),('event_msg',{'type':'agent_message','message':self.output}),('event_msg',{'type':'task_complete','turn_id':'turn','last_agent_message':self.output})]
        observations=[('event_msg',{'type':'private_s05_model_observed','turn_id':m['turnId'],'response_id':m['responseId'],'model':m['model'],'source':m['source']}) for m in [self.model,*self.additional_models]]
        events=events[:6]+observations+events[7:]
        path=self.home/'rollout.jsonl';path.write_text(''.join(json.dumps({'type':t,'payload':p})+'\n' for t,p in events));path.chmod(0o600)
    def wait_notification(self,*args,**kwargs):return self.completion
    def seal_notifications(self,timeout):
        self.sealed=True
        trace=[{'method':'thread/status/changed','params':{'threadId':'thread','status':{'type':'active','activeFlags':[]}}},{'method':'thread/started','params':{'thread':{'id':'thread'}}},{'method':'private/s05/modelObserved','params':self.model},{'method':'private/s05/outboundObserved','params':self.out},{'method':'item/completed','params':{'threadId':'thread','turnId':'turn','item':{'type':'userMessage','content':[{'type':'text','text':self.prompt,'text_elements':[]}]}}},{'method':'item/completed','params':{'threadId':'thread','turnId':'turn','item':{'type':'agentMessage','text':self.output}}},self.completion]
        return trace[:2]+[{'method':'private/s05/modelObserved','params':m} for m in [self.model,*self.additional_models]]+trace[3:]

class AdapterTests(unittest.TestCase):
    def setUp(self):
        self.temp=tempfile.TemporaryDirectory(dir='/private/tmp');self.home=Path(self.temp.name);self.home.chmod(0o700)
        ns={'__name__':'fixture'};exec(CHECKER.read_bytes(),ns)
        self.prompt=ns['build_prompt'](CASE_NONCE);self.output=json.dumps(ns['expected_result'](CASE_NONCE))
        ledger=self.home/'ledger';ledger.mkdir(mode=0o700);self.ledger=PrivateSealLedger(ledger)
        self.ledger.reserve(invocation_key='key',intent_digest='a'*64,prompt_sha256='b'*64,source_binding_sha256='c'*64,profile_sha256='d'*64)
        self.rpc=FakeRpc(self.home,self.prompt,self.output)
    def tearDown(self):self.temp.cleanup()
    def run_flow(self):return observe_private(self.rpc,prompt=self.prompt,cwd=str(self.home),invocation_key='key',seal_ledger=self.ledger,intent_digest='a'*64)
    def test_complete_fake_workflow_closes_before_readback_and_server_effort_unknown(self):
        result=self.run_flow();self.assertTrue(self.rpc.sealed);self.assertEqual(result['server_reported_effort'],'UNKNOWN');self.assertEqual(result['semantic_correctness'],'passed')
        self.assertEqual([x[0] for x in self.rpc.calls],['thread/start','private/s05/session.read','turn/start','private/s05/history.read'])
        self.assertEqual(set(self.rpc.calls[0][1]),{'model','modelProvider','ephemeral','allowProviderModelFallback'})
    def test_passive_actual02_notices_preserved_in_complete_ordinary_seal(self):
        notices=copy.deepcopy(ACTUAL02_PASSIVE_NOTICES);original=self.rpc.seal_notifications
        self.rpc.seal_notifications=lambda timeout:notices[:1]+original(timeout)+notices[1:]
        result=self.run_flow()
        self.assertEqual(result['complete_notification_trace'][0],notices[0])
        self.assertEqual(result['complete_notification_trace'][-1],notices[1])
        self.assertEqual(result['actual_serving_model'],'gpt-6-luna')
        self.assertEqual(result['server_reported_effort'],'UNKNOWN')
        self.assertEqual(result['semantic_correctness'],'passed')
    def test_null_allowed_tools_denied_before_send(self):
        self.rpc.captured['allowedTools']=None
        with self.assertRaises(PrivateS05EvidenceRejected):self.run_flow()
        self.assertEqual(len(self.rpc.calls),2)
    def test_conflicting_actual_model_denied(self):
        self.rpc.model['model']='gpt-6.1-sol'
        with self.assertRaises(PrivateS05EvidenceRejected):self.run_flow()
    def test_missing_actual_model_denied(self):
        original=self.rpc.seal_notifications
        self.rpc.seal_notifications=lambda timeout:[n for n in original(timeout) if n['method']!='private/s05/modelObserved']
        with self.assertRaises(PrivateS05EvidenceRejected):self.run_flow()
    def test_foreign_thread_denied(self):
        self.rpc.model['threadId']='foreign'
        with self.assertRaises(PrivateS05EvidenceRejected):self.run_flow()
    def test_null_serialized_tools_denied(self):
        self.rpc.out['toolNames']=None
        with self.assertRaises(PrivateS05EvidenceRejected):self.run_flow()
    def test_multiple_serialized_requests_denied(self):
        original=self.rpc.seal_notifications
        self.rpc.seal_notifications=lambda timeout:original(timeout)+[{'method':'private/s05/outboundObserved','params':self.rpc.out}]
        with self.assertRaises(PrivateS05EvidenceRejected):self.run_flow()
    def test_late_tool_event_after_completion_denied(self):
        original=self.rpc.seal_notifications
        self.rpc.seal_notifications=lambda timeout:original(timeout)+[{'method':'item/completed','params':{'threadId':'thread','turnId':'turn','item':{'type':'commandExecution'}}}]
        with self.assertRaises(PrivateS05EvidenceRejected):self.run_flow()
    def test_history_read_side_effect_denied(self):
        original=self.rpc.request
        def request(method,params,timeout=None):
            result=original(method,params)
            if method=='private/s05/history.read':
                with (self.home/'rollout.jsonl').open('a') as f:f.write('\n')
            return result
        self.rpc.request=request
        with self.assertRaises(PrivateS05EvidenceRejected):self.run_flow()
    def test_source_revalidation_failure_prevents_turn_send(self):
        def deny():raise PrivateS05EvidenceRejected('changed current source')
        with self.assertRaises(PrivateS05EvidenceRejected):observe_private(self.rpc,prompt=self.prompt,cwd=str(self.home),invocation_key='key',seal_ledger=self.ledger,intent_digest='a'*64,before_send=deny)
        self.assertEqual(len(self.rpc.calls),2)
    def test_provider_retry_nonzero_denied(self):
        self.rpc.captured['requestMaxRetries']=1
        with self.assertRaises(PrivateS05EvidenceRejected):self.run_flow()
        self.assertEqual(len(self.rpc.calls),2)
    def test_unsealed_capture_denied(self):
        self.rpc.seal_notifications=lambda timeout:(_ for _ in ()).throw(PrivateS05EvidenceRejected('incomplete EOF'))
        with self.assertRaises(PrivateS05EvidenceRejected):self.run_flow()
    def test_mismatched_saved_output_after_shutdown_denied(self):
        original=self.rpc.seal_notifications
        def seal(timeout):
            result=original(timeout);self.rpc.output='changed';self.rpc.write();return result
        self.rpc.seal_notifications=seal
        with self.assertRaises(PrivateS05EvidenceRejected):self.run_flow()
    def startup_fixture(self):
        return json.loads((Path(__file__).parent/'fixtures/private_s05_disabled_startup_notice.json').read_text())
    def test_actual_disabled_startup_notice_retained(self):
        original=self.rpc.seal_notifications;notice=self.startup_fixture()
        self.rpc.seal_notifications=lambda timeout:[notice]+original(timeout)
        result=self.run_flow();self.assertEqual(result['complete_notification_trace'][0],notice)
    def test_enabled_startup_notice_denied(self):
        original=self.rpc.seal_notifications;notice=self.startup_fixture();notice['params']['status']='connected'
        self.rpc.seal_notifications=lambda timeout:[notice]+original(timeout)
        with self.assertRaises(PrivateS05EvidenceRejected):self.run_flow()
    def test_environment_bound_startup_notice_denied(self):
        original=self.rpc.seal_notifications;notice=self.startup_fixture();notice['params']['environmentId']='foreign'
        self.rpc.seal_notifications=lambda timeout:[notice]+original(timeout)
        with self.assertRaises(PrivateS05EvidenceRejected):self.run_flow()
    def test_late_disabled_notice_denied(self):
        original=self.rpc.seal_notifications;notice=self.startup_fixture()
        self.rpc.seal_notifications=lambda timeout:original(timeout)+[notice]
        with self.assertRaises(PrivateS05EvidenceRejected):self.run_flow()
    def test_missing_extra_and_duplicate_disabled_notices_denied(self):
        from scripts.private_s05_adapter import check_disabled_startup_notice
        notice=self.startup_fixture()
        for key in list(notice['params']):
            broken=copy.deepcopy(notice);del broken['params'][key]
            with self.assertRaises(PrivateS05EvidenceRejected):check_disabled_startup_notice(broken,startup_phase=True,already_seen=False)
        broken=copy.deepcopy(notice);broken['params']['unknownSemanticField']=False
        with self.assertRaises(PrivateS05EvidenceRejected):check_disabled_startup_notice(broken,startup_phase=True,already_seen=False)
        broken=copy.deepcopy(notice);broken['unknownEnvelopeField']=False
        with self.assertRaises(PrivateS05EvidenceRejected):check_disabled_startup_notice(broken,startup_phase=True,already_seen=False)
        with self.assertRaises(PrivateS05EvidenceRejected):check_disabled_startup_notice(notice,startup_phase=True,already_seen=True)
    def configuration_fixtures(self):
        from scripts.private_s05_adapter import expected_skill_warning
        notices=json.loads((Path(__file__).parent/'fixtures/private_s05_configuration_notices04.json').read_text())
        notices[1]['params']={'threadId':'thread','message':expected_skill_warning(str(self.home))}
        return notices
    def test_exact_source_configuration_notices_retained_before_turn(self):
        original=self.rpc.seal_notifications;notices=self.configuration_fixtures()
        def seal(timeout):
            trace=original(timeout);return trace[:2]+notices+trace[2:]
        self.rpc.seal_notifications=seal
        result=self.run_flow();self.assertEqual(result['complete_notification_trace'][2:4],notices)
    def test_configuration_notice_foreign_text_shape_path_and_duplicates_denied(self):
        from scripts.private_s05_adapter import check_configuration_notice
        originals=self.configuration_fixtures()
        broken=[]
        for notice in originals:
            for key in notice['params']:
                n=copy.deepcopy(notice);del n['params'][key];broken.append(n)
            n=copy.deepcopy(notice);n['params']['extra']=None;broken.append(n)
            n=copy.deepcopy(notice);n['extra']=False;broken.append(n)
        for field,value in [('summary','other feature'),('details',None)]:
            n=copy.deepcopy(originals[0]);n['params'][field]=value;broken.append(n)
        for field,value in [('threadId','foreign'),('message',originals[1]['params']['message'].replace(str(self.home),'/Users/tony/.codex')),('message',originals[1]['params']['message'].replace('skip_host_skill_discovery','other_feature'))]:
            n=copy.deepcopy(originals[1]);n['params'][field]=value;broken.append(n)
        for n in broken:
            with self.subTest(notice=n),self.assertRaises(PrivateS05EvidenceRejected):check_configuration_notice(n,thread='thread',cwd=str(self.home),pre_turn=True,seen=set())
        for n in originals:
            with self.assertRaises(PrivateS05EvidenceRejected):check_configuration_notice(n,thread='thread',cwd=str(self.home),pre_turn=True,seen={n['method']})
    def test_each_configuration_notice_after_first_turn_metadata_denied(self):
        from scripts.private_s05_adapter import trace_check
        for notice in self.configuration_fixtures():
            trace=self.rpc.seal_notifications(30);self.rpc.sealed=False
            trace.insert(3,notice)
            with self.assertRaises(PrivateS05EvidenceRejected):trace_check(trace,'thread','turn',self.rpc.completion,self.prompt,self.output,str(self.home))
    def test_completed_object_source_preserved_through_live_committed_and_raw_history(self):
        self.rpc.model['source']='remoteResponseObjectModel'
        result=self.run_flow();self.assertEqual(result['model_observations'][0]['source'],'remoteResponseObjectModel');self.assertEqual(result['model_evidence_level'],'provider_declared_used_model_id');self.assertEqual(result['internal_backend_model'],'UNKNOWN');self.assertEqual(result['server_reported_effort'],'UNKNOWN')
        raw=(self.home/'rollout.jsonl').read_text();self.assertIn('remoteResponseObjectModel',raw)
    def test_object_and_matching_header_preserved_without_suppression(self):
        self.rpc.model['source']='remoteResponseObjectModel';self.rpc.additional_models=[{**self.rpc.model,'source':'httpOpenAIModelHeader','responseId':None}]
        result=self.run_flow();self.assertEqual(result['model_observations'],[self.rpc.model,*self.rpc.additional_models])
    def test_object_missing_id_unknown_source_and_any_header_contradiction_denied(self):
        from scripts.private_s05_adapter import telemetry
        base={**self.rpc.model,'source':'remoteResponseObjectModel'}
        for value in (None,'',' ',42,' resp '):
            with self.assertRaises(PrivateS05EvidenceRejected):telemetry([{**base,'responseId':value}],[self.rpc.out],'thread','turn')
        for field in ('threadId','turnId'):
            with self.assertRaises(PrivateS05EvidenceRejected):telemetry([{**base,field:'foreign'}],[self.rpc.out],'thread','turn')
        missing={k:v for k,v in base.items() if k!='responseId'}
        with self.assertRaises(PrivateS05EvidenceRejected):telemetry([missing],[self.rpc.out],'thread','turn')
        for source in ('requested.model','configured.model','assistantGeneratedText','response.created','unknownRemoteMetadata'):
            with self.assertRaises(PrivateS05EvidenceRejected):telemetry([{**base,'source':source}],[self.rpc.out],'thread','turn')
        for field,value in [('model','other-model'),('responseId','foreign-response')]:
            header={**base,'source':'sseResponseHeaders',field:value}
            for models in ([base,header],[header,base]):
                with self.assertRaises(PrivateS05EvidenceRejected):telemetry(models,[self.rpc.out],'thread','turn')
    def test_object_live_and_committed_response_id_mismatch_denied(self):
        self.rpc.model['source']='remoteResponseObjectModel';original=self.rpc.seal_notifications
        def seal(timeout):
            trace=copy.deepcopy(original(timeout))
            for notice in trace:
                if notice['method']=='private/s05/modelObserved':notice['params']['responseId']='different-live-response'
            return trace
        self.rpc.seal_notifications=seal
        with self.assertRaises(PrivateS05EvidenceRejected):self.run_flow()
    def test_wrong_semantics_and_nonce_denied(self):
        with self.assertRaises(PrivateS05EvidenceRejected):semantic_check(self.prompt,'{}')
        with self.assertRaises(PrivateS05EvidenceRejected):semantic_check(self.prompt+'x',self.output)


ACTUAL02_PASSIVE_NOTICES=[{'emittedAtMs': 1791279343181, 'method': 'account/updated', 'params': {'authMode': 'chatgpt', 'planType': 'pro'}}, {'emittedAtMs': 1791279349765, 'method': 'account/rateLimits/updated', 'params': {'rateLimits': {'credits': {'balance': '56809.2100680000', 'hasCredits': True, 'unlimited': False}, 'individualLimit': None, 'limitId': 'codex', 'limitName': None, 'normalModelSlug': None, 'planType': 'pro', 'primary': {'resetsAt': 1791863015, 'usedPercent': 24, 'windowDurationMins': 10080}, 'rateLimitReachedType': None, 'secondary': None, 'spendControlReached': None}}}]

class PassiveAccountDtoTests(unittest.TestCase):
    def check(self,n):
        from scripts.private_s05_adapter import check_passive_account_notice
        check_passive_account_notice(n)
    def test_actual02_exact_dtos(self):
        for n in ACTUAL02_PASSIVE_NOTICES:self.check(n)
    def test_nullable_nested_variants_and_full_integer_width(self):
        from scripts.private_s05_adapter import AUTH_MODES,PLAN_TYPES,REACHED_TYPES
        n=copy.deepcopy(ACTUAL02_PASSIVE_NOTICES[0]);n['params']={'authMode':None,'planType':None};self.check(n)
        for mode in AUTH_MODES:n['params']['authMode']=mode;self.check(n)
        for plan in PLAN_TYPES:n['params']['planType']=plan;self.check(n)
        n=copy.deepcopy(ACTUAL02_PASSIVE_NOTICES[1]);r=n['params']['rateLimits']
        for key in r:r[key]=None
        self.check(n)
        r['secondary']={'usedPercent':-2**31,'windowDurationMins':-2**63,'resetsAt':2**63-1}
        r['individualLimit']={'limit':'10','used':'2','remainingPercent':2**31-1,'resetsAt':-2**63}
        r['spendControlReached']=False;self.check(n)
        for value in REACHED_TYPES:r['rateLimitReachedType']=value;self.check(n)
    def test_malformed_shapes_enums_types_widths_and_local_utf8_bounds(self):
        changes=[lambda n:n.update(extra=True),lambda n:n.pop('params'),lambda n:n.update(method='account/unknown'),lambda n:n['params'].update(threadId='foreign'),lambda n:n['params'].update(authMode='invented'),lambda n:n['params'].update(planType=3),lambda n:n.update(emittedAtMs=True),lambda n:n.update(emittedAtMs=2**63)]
        for change in changes:
            n=copy.deepcopy(ACTUAL02_PASSIVE_NOTICES[0]);change(n)
            with self.assertRaises(PrivateS05EvidenceRejected):self.check(n)
        changes=[lambda r:r.pop('primary'),lambda r:r.update(extra=0),lambda r:r.update(primary=[]),lambda r:r['primary'].update(usedPercent=True),lambda r:r['primary'].update(usedPercent=2**31),lambda r:r['primary'].update(resetsAt=2**63),lambda r:r['credits'].update(hasCredits=1),lambda r:r.update(spendControlReached=1),lambda r:r.update(rateLimitReachedType='invented'),lambda r:r.update(limitName='é'*513),lambda r:r.update(limitName='\ud800'),lambda r:r.update(individualLimit={'limit':'1','used':'0','remainingPercent':0})]
        for change in changes:
            n=copy.deepcopy(ACTUAL02_PASSIVE_NOTICES[1]);change(n['params']['rateLimits'])
            with self.assertRaises(PrivateS05EvidenceRejected):self.check(n)
    def test_local_total_size_bound(self):
        n=copy.deepcopy(ACTUAL02_PASSIVE_NOTICES[1]);n['params']['rateLimits']['limitName']='x'*1024
        # Each string is individually bounded; serialized repeated nesting is not.
        n['params']['rateLimits']['primary']={str(i):'x'*1024 for i in range(20)}
        with self.assertRaisesRegex(PrivateS05EvidenceRejected,'exceeds local UTF8 bound'):self.check(n)
    def test_original_duplicate_configuration_warning_still_rejected(self):
        from scripts.private_s05_adapter import check_configuration_notice,expected_skill_warning
        n={'method':'warning','params':{'threadId':'thread','message':expected_skill_warning('/private/owned')}};seen=set()
        check_configuration_notice(n,thread='thread',cwd='/private/owned',pre_turn=True,seen=seen)
        with self.assertRaisesRegex(PrivateS05EvidenceRejected,'late or duplicated'):
            check_configuration_notice(n,thread='thread',cwd='/private/owned',pre_turn=True,seen=seen)

if __name__=='__main__':unittest.main()
