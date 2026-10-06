import copy,json,unittest
from scripts.private_s05_evidence import ExpectedPrivateTurn,PrivateS05EvidenceRejected,check_private_evidence,compare_passive_read

class PrivateEvidenceTests(unittest.TestCase):
    def setUp(self):
        self.expected=ExpectedPrivateTurn('owned-thread','owned-turn','fresh-key','exact task','/owned/runtime')
        ids={'thread_id':'owned-thread','turn_id':'owned-turn'}
        self.session={**ids,'provider':'openai','model':'gpt-6-luna','effort':'xhigh','approval_policy':'never','sandbox':'read-only','cwd':'/owned/runtime','ephemeral':False,'fallback_enabled':False,'tool_policy_allowed_tools':[],'final_tool_specs':[]}
        self.outbound=[{**ids,'provider':'openai','model':'gpt-6-luna','effort':'xhigh','tools':[],'response_id':'provider-response'}]
        self.serving=[{**ids,'response_id':'provider-response','model':'gpt-6-luna','source':'http.openai-model'}]
        self.history={**ids,'status':'completed','error':None,'items_view':'full','items':[{'type':'userMessage','text':'exact task','invocation_key':'fresh-key'},{'type':'agentMessage','text':'actual response'}]}
    def check(self):return check_private_evidence(self.expected,session=self.session,outbound=self.outbound,serving=self.serving,history_raw=json.dumps(self.history).encode())
    def test_complete_observed_evidence_passes_and_server_effort_unknown(self):
        got=self.check();self.assertEqual(got.response_text,'actual response');self.assertIsNone(got.server_reported_effort);self.assertEqual(got.serialized_request_effort,'xhigh')
    def test_foreign_thread_and_turn_denied(self):
        for part in (self.session,self.outbound[0],self.serving[0],self.history):
            for field in ('thread_id','turn_id'):
                old=part[field];part[field]='foreign'
                with self.assertRaises(PrivateS05EvidenceRejected):self.check()
                part[field]=old
    def test_missing_remote_model_denied(self):
        self.serving=[]
        with self.assertRaises(PrivateS05EvidenceRejected):self.check()
    def test_conflicting_remote_model_denied(self):
        self.serving.append({**self.serving[0],'model':'different-model'})
        with self.assertRaises(PrivateS05EvidenceRejected):self.check()
    def test_configured_model_cannot_replace_provider_header(self):
        self.serving[0]['source']='configured.model'
        with self.assertRaises(PrivateS05EvidenceRejected):self.check()
    def test_no_tools_not_assumed_from_policy(self):
        self.session['final_tool_specs']=['hidden-tool']
        with self.assertRaises(PrivateS05EvidenceRejected):self.check()
    def test_no_tools_not_assumed_from_empty_specs(self):
        self.session['tool_policy_allowed_tools']=None
        with self.assertRaises(PrivateS05EvidenceRejected):self.check()
    def test_serialized_effort_tools_and_provider_response_binding_required(self):
        for key,value in [('effort','medium'),('tools',['hidden-tool']),('response_id','foreign-response')]:
            old=self.outbound[0][key];self.outbound[0][key]=value
            with self.assertRaises(PrivateS05EvidenceRejected):self.check()
            self.outbound[0][key]=old
    def test_second_outbound_request_denied(self):
        self.outbound.append(copy.deepcopy(self.outbound[0]))
        with self.assertRaises(PrivateS05EvidenceRejected):self.check()
    def test_saved_prompt_identity_and_tool_items_denied(self):
        for key,value in [('text','other task'),('invocation_key','replayed-key')]:
            old=self.history['items'][0][key];self.history['items'][0][key]=value
            with self.assertRaises(PrivateS05EvidenceRejected):self.check()
            self.history['items'][0][key]=old
        self.history['items'].append({'type':'commandExecution'})
        with self.assertRaises(PrivateS05EvidenceRejected):self.check()
    def test_partial_or_nonterminal_history_denied(self):
        for key,value in [('items_view','summary'),('status','inProgress'),('error',{})]:
            old=self.history[key];self.history[key]=value
            with self.assertRaises(PrivateS05EvidenceRejected):self.check()
            self.history[key]=old
    def test_completed_remote_object_used_id_and_six_headers_remain_supported(self):
        headers={'httpOpenAIModelHeader','websocketHandshakeOpenAIModelHeader','sseResponseHeaders','sseTopLevelHeaders','websocketResponseHeaders','websocketTopLevelHeaders'}
        for source in headers|{'remoteResponseObjectModel'}:
            self.serving[0]['source']=source
            result=self.check();self.assertEqual(result.model_evidence_level,'provider_declared_used_model_id');self.assertEqual(result.internal_backend_model,'UNKNOWN');self.assertIsNone(result.server_reported_effort)
            self.assertEqual(result.serving_model_sources,(source,))
    def test_object_id_absent_blank_or_malformed_and_self_report_denied(self):
        self.serving[0]['source']='remoteResponseObjectModel'
        for value in (None,'',' ',7,[], ' response '):
            self.serving[0]['response_id']=value
            with self.assertRaises(PrivateS05EvidenceRejected):self.check()
        self.serving[0]['response_id']='provider-response'
        for source in ('configured.model','requested.model','assistantGeneratedText','response.created','remoteResponseObjectModelTypo'):
            self.serving[0]['source']=source
            with self.assertRaises(PrivateS05EvidenceRejected):self.check()
        self.serving=[];self.history['items'][1]['text']='I used gpt-6-luna'
        with self.assertRaises(PrivateS05EvidenceRejected):self.check()
    def test_header_object_model_and_known_response_conflicts_rejected_in_any_order(self):
        base={**self.serving[0],'source':'remoteResponseObjectModel'}
        self.outbound[0].pop('response_id')
        header={**base,'source':'httpOpenAIModelHeader'}
        self.serving=[base,header];self.check()
        self.serving=[base,{**header,'response_id':None}];self.check()
        for field,value in [('model','different-model'),('response_id','another-response')]:
            conflict={**header,field:value}
            for records in ([base,conflict],[conflict,base]):
                self.serving=records
                with self.assertRaises(PrivateS05EvidenceRejected):self.check()
    def test_passive_read_requires_independent_identical_storage_snapshots(self):
        digest=self.check().history_sha256;snapshot={'history_sha256':digest,'inode':1,'mtime_ns':2,'files':1}
        compare_passive_read(snapshot,dict(snapshot),expected_history_sha256=digest)
        for change in ({**snapshot,'mtime_ns':3},{**snapshot,'files':2}):
            with self.assertRaises(PrivateS05EvidenceRejected):compare_passive_read(snapshot,change,expected_history_sha256=digest)
if __name__=='__main__':unittest.main()
