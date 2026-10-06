"""Owned S05 wire adapter. Internal composition only; never caller JSON evidence."""
from pathlib import Path
import hashlib,time
RPC_TIMEOUT_SECONDS=30.0
COMPLETION_TIMEOUT_SECONDS=180.0
SEAL_TIMEOUT_SECONDS=30.0
from scripts.caller_host_routing import _json
from scripts.private_s05_evidence import ExpectedPrivateTurn,check_private_evidence,PrivateS05EvidenceRejected
from scripts.private_s05_rollout import MODEL_SOURCES,stable_rollout,parse_rollout,deny
from scripts.private_s05_seal import OneFreshPrivateTurn


def exact(obj,fields):
    if not isinstance(obj,dict) or set(obj)!=set(fields.split()):deny('unexpected private wire shape')
    return obj

def digest(value):
    if not isinstance(value,str) or len(value)!=64 or any(x not in '0123456789abcdef' for x in value):deny('missing private telemetry digest')
    return value

def bound(obj,thread,turn=None):
    if obj.get('threadId')!=thread or (turn is not None and obj.get('turnId')!=turn):deny('foreign private telemetry identity')

def session_projection(raw,thread,cwd):
    exact(raw,'threadId capturedSession enforcedDispatch source');bound(raw,thread)
    if raw['source']!='ownedThreadSession' or raw['enforcedDispatch']!={'allowProviderModelFallback':False,'source':'ownedThreadStartGuard'}:deny('private session dispatch provenance differs')
    c=exact(raw['capturedSession'],'model modelProvider effort approvalPolicy sandboxPolicy cwd allowedTools ephemeral requestMaxRetries streamMaxRetries source')
    if c['source']!='capturedSessionConfigurationAndToolPolicy':deny('missing captured session provenance')
    if c['sandboxPolicy'] not in ({'type':'read-only'},{'type':'read-only','network_access':False}):deny('captured sandbox is not read-only')
    if c['allowedTools']!=[] or c['ephemeral'] is not False or type(c['requestMaxRetries']) is not int or c['requestMaxRetries']!=0 or type(c['streamMaxRetries']) is not int or c['streamMaxRetries']!=0:deny('private policy or provider retry bound differs')
    required={'model':'gpt-6-luna','modelProvider':'openai','effort':'xhigh','approvalPolicy':'never','cwd':cwd}
    if any(c[k]!=v for k,v in required.items()):deny('captured session differs from exact S05 profile')
    return c

def telemetry(models,outbounds,thread,turn):
    if not isinstance(models,list) or not models or not isinstance(outbounds,list) or len(outbounds)!=1:deny('missing model proof or multiple serialized requests')
    known_response_ids=set()
    for m in models:
        exact(m,'threadId turnId responseId model source');bound(m,thread,turn)
        if m['model']!='gpt-6-luna' or m['source'] not in MODEL_SOURCES or (m['responseId'] is not None and (not isinstance(m['responseId'],str) or not m['responseId'].strip() or m['responseId']!=m['responseId'].strip())):deny('conflicting or invalid serving model proof')
        if m['source']=='remoteResponseObjectModel' and m['responseId'] is None:deny('completed remote response object requires actual response identity')
        if m['responseId'] is not None:known_response_ids.add(m['responseId'])
        if len(known_response_ids)>1:deny('conflicting provider model response identities')
    o=exact(outbounds[0],'threadId turnId model effort toolNames toolsSha256 requestSha256 source');bound(o,thread,turn)
    if (o['model'],o['effort'],o['toolNames'],o['source'])!=('gpt-6-luna','xhigh',[],'serializedResponsesRequest'):deny('unknown or conflicting outbound proof')
    digest(o['toolsSha256']);digest(o['requestSha256'])
    # Host hashes actual final serialized tool array, not a requested policy.
    if o['toolsSha256']!=hashlib.sha256(b'[]').hexdigest():deny('serialized empty tools digest differs')

def check_disabled_startup_notice(notice,*,startup_phase,already_seen):
    if not startup_phase or already_seen:deny('remote-control notice is late or duplicated')
    if not isinstance(notice,dict) or not {'method','params'}.issubset(notice) or set(notice)-{'method','params','emittedAtMs'} or notice['method']!='remoteControl/status/changed':deny('unknown startup remote-control envelope')
    if 'emittedAtMs' in notice and (type(notice['emittedAtMs']) is not int or notice['emittedAtMs']<0):deny('invalid startup notification timestamp')
    p=exact(notice.get('params'),'status serverName installationId environmentId')
    if p['status']!='disabled' or p['environmentId'] is not None:deny('remote control is active unknown or environment-bound')
    for key in ('serverName','installationId'):
        if not isinstance(p[key],str) or not p[key].strip() or len(p[key].encode())>512:deny('missing bounded startup remote-control identity')
    return True

DEPRECATION_SUMMARY="`[features].codex_hooks` is deprecated. Use `[features].hooks` instead."
DEPRECATION_DETAILS="Enable it with `--enable hooks` or `[features].hooks` in config.toml. See https://developers.openai.com/codex/config-basic#feature-flags for details."
def expected_skill_warning(cwd):
    return "Under-development features enabled: skip_host_skill_discovery. Under-development features are incomplete and may behave unpredictably. To suppress this warning, set `suppress_unstable_features_warning = true` in "+str(Path(cwd)/'config.toml')+"."

def check_configuration_notice(notice,*,thread,cwd,pre_turn,seen):
    # Exact private fixed overrides, not a general warning/deprecation exemption.
    method=notice.get('method') if isinstance(notice,dict) else None
    if not pre_turn or method in seen:deny('configuration notice is late or duplicated')
    if method not in {'deprecationNotice','warning'} or not {'method','params'}.issubset(notice) or set(notice)-{'method','params','emittedAtMs'}:deny('unknown configuration notice envelope')
    if 'emittedAtMs' in notice and (type(notice['emittedAtMs']) is not int or notice['emittedAtMs']<0):deny('invalid configuration notification timestamp')
    if method=='deprecationNotice':
        p=exact(notice['params'],'summary details')
        if p!={'summary':DEPRECATION_SUMMARY,'details':DEPRECATION_DETAILS}:deny('unexpected feature deprecation')
    else:
        p=exact(notice['params'],'threadId message')
        if p!={'threadId':thread,'message':expected_skill_warning(cwd)}:deny('foreign or unexpected private configuration warning')
    seen.add(method)

# Passive upstream DTO metadata only. These LOCAL bounds do not assert
# upstream string/percentage constraints or authorize account/config changes.
PASSIVE_ACCOUNT_METHODS={'account/updated','account/rateLimits/updated'}
AUTH_MODES={'apikey','chatgpt','chatgptAuthTokens','headers','agentIdentity','personalAccessToken','bedrockApiKey','bedrockAccessKeys'}
PLAN_TYPES={'free','go','plus','pro','prolite','promax','team','self_serve_business_prolite','self_serve_business_usage_based','business','ent26','enterprise_cbp_automation','enterprise_cbp_usage_based','enterprise','edu','edu_plus','edu_pro','unknown'}
REACHED_TYPES={'rate_limit_reached','workspace_owner_credits_depleted','workspace_member_credits_depleted','workspace_owner_usage_limit_reached','workspace_member_usage_limit_reached'}

def check_passive_account_notice(notice):
    if not isinstance(notice,dict) or not {'method','params'}.issubset(notice) or set(notice)-{'method','params','emittedAtMs'}:deny('unexpected passive account envelope')
    if notice['method'] not in PASSIVE_ACCOUNT_METHODS:deny('unknown passive account method')
    if 'emittedAtMs' in notice and (type(notice['emittedAtMs']) is not int or not 0<=notice['emittedAtMs']<=2**63-1):deny('invalid passive account timestamp')
    def strings(value):
        if isinstance(value,str):
            if len(value.encode('utf-8'))>1024:deny('passive account string exceeds local UTF8 bound')
        elif isinstance(value,dict):
            for k,v in value.items():strings(k);strings(v)
        elif isinstance(value,list):
            for v in value:strings(v)
    try:
        strings(notice)
        if len(_json(notice).encode('utf-8'))>16384:deny('passive account notice exceeds local UTF8 bound')
    except PrivateS05EvidenceRejected:raise
    except (UnicodeError,TypeError,ValueError):deny('invalid passive account serialization')
    def integer(v,bits):
        if type(v) is not int or not -(2**(bits-1))<=v<2**(bits-1):deny('invalid passive account integer')
    def boolean(v):
        if type(v) is not bool:deny('invalid passive account boolean')
    def string(v):
        if not isinstance(v,str):deny('invalid passive account string')
    def enum(v,values):
        if type(v) is not str or v not in values:deny('unknown passive account enum')
    def optional(v,check):
        if v is not None:check(v)
    p=notice['params']
    if notice['method']=='account/updated':
        exact(p,'authMode planType');optional(p['authMode'],lambda v:enum(v,AUTH_MODES));optional(p['planType'],lambda v:enum(v,PLAN_TYPES));return
    exact(p,'rateLimits');r=exact(p['rateLimits'],'limitId limitName normalModelSlug primary secondary credits individualLimit spendControlReached planType rateLimitReachedType')
    for key in ('limitId','limitName','normalModelSlug'):optional(r[key],string)
    def window(v):
        exact(v,'usedPercent windowDurationMins resetsAt');integer(v['usedPercent'],32)
        for key in ('windowDurationMins','resetsAt'):optional(v[key],lambda x:integer(x,64))
    optional(r['primary'],window);optional(r['secondary'],window)
    def credits(v):
        exact(v,'hasCredits unlimited balance');boolean(v['hasCredits']);boolean(v['unlimited']);optional(v['balance'],string)
    optional(r['credits'],credits)
    def individual(v):
        exact(v,'limit used remainingPercent resetsAt');string(v['limit']);string(v['used']);integer(v['remainingPercent'],32);integer(v['resetsAt'],64)
    optional(r['individualLimit'],individual);optional(r['spendControlReached'],boolean);optional(r['planType'],lambda v:enum(v,PLAN_TYPES));optional(r['rateLimitReachedType'],lambda v:enum(v,REACHED_TYPES))

def trace_check(trace,thread,turn,completion,prompt,output,cwd):
    models=[];outbounds=[];completed=[];users=[];agents=[];startup_phase=True;remote_seen=False;pre_turn=True;configuration_seen=set()
    allowed={'thread/started','thread/status/changed','turn/started','turn/completed','item/started','item/completed','item/agentMessage/delta','item/reasoning/textDelta','item/reasoning/summaryTextDelta','item/reasoning/summaryPartAdded','thread/tokenUsage/updated','private/s05/modelObserved','private/s05/outboundObserved'}
    for n in trace:
        method=n.get('method');p=n.get('params')
        if method in PASSIVE_ACCOUNT_METHODS:
            check_passive_account_notice(n)
            continue
        if method=='remoteControl/status/changed':
            remote_seen=check_disabled_startup_notice(n,startup_phase=startup_phase,already_seen=remote_seen)
            continue
        if method in {'deprecationNotice','warning'}:
            check_configuration_notice(n,thread=thread,cwd=cwd,pre_turn=pre_turn,seen=configuration_seen)
            continue
        startup_phase=False
        if method not in {'thread/started','thread/status/changed'} or (isinstance(p,dict) and 'turnId' in p):pre_turn=False
        if method not in allowed or not isinstance(p,dict):deny('tool reroute or unknown notification in complete private trace')
        observed_thread=p.get('thread',{}).get('id') if method=='thread/started' else p.get('threadId')
        if observed_thread!=thread:deny('foreign private notification')
        if method=='thread/status/changed' and p.get('status') not in ({'type':'idle'},{'type':'active','activeFlags':[]}):deny('private thread status indicates tools approval input or error')
        if method in {'turn/started','turn/completed'} and p.get('turn',{}).get('id')!=turn:deny('foreign private turn snapshot')
        if 'turnId' in p and p['turnId']!=turn:deny('foreign private turn notification')
        if method.startswith('item/') and method in {'item/started','item/completed'}:
            if not isinstance(p.get('item'),dict) or p['item'].get('type') not in {'userMessage','agentMessage','reasoning'}:deny('tool or unknown item in private trace')
        if method=='item/completed':
            item=p['item']
            if item['type']=='userMessage':users.append(item.get('content'))
            elif item['type']=='agentMessage':agents.append(item.get('text'))
        if method=='turn/completed':completed.append(n)
        elif method=='private/s05/modelObserved':models.append(p)
        elif method=='private/s05/outboundObserved':outbounds.append(p)
    if completed!=[completion]:deny('completed trace differs or contains another completion')
    if len(users)!=1 or not isinstance(users[0],list) or len(users[0])!=1 or users[0][0].get('type')!='text' or users[0][0].get('text')!=prompt or users[0][0].get('text_elements',[])!=[] or not agents or agents[-1]!=output:deny('sealed live prompt or output differs from saved turn')
    telemetry(models,outbounds,thread,turn)
    return models,outbounds

def snapshots(home):
    paths=list(Path(home).rglob('*.jsonl'))
    if len(paths)>2:deny('unexpected owned rollout inventory')
    return {str(p):stable_rollout(p,home)[1] for p in paths}

def observe_private(rpc,*,prompt,cwd,invocation_key,seal_ledger,intent_digest,before_send=None):
    observation_started=time.monotonic()
    observed={'stage':'thread_start'};rpc._private_s05_observation=observed
    fence=OneFreshPrivateTurn();fence.claim_start()
    started=rpc.request('thread/start',{'model':'gpt-6-luna','modelProvider':'openai','ephemeral':False,'allowProviderModelFallback':False},timeout=RPC_TIMEOUT_SECONDS)
    observed.update(thread_start_result=started,stage='session_read')
    thread=started.get('thread',{}).get('id');fence.record_thread(thread)
    session_observation=rpc.request('private/s05/session.read',{'threadId':thread},timeout=RPC_TIMEOUT_SECONDS)
    observed.update(session_observation=session_observation,stage='session_projection')
    captured=session_projection(session_observation,thread,cwd)
    observed.update(captured_session=captured,stage='current_source_before_send')
    if before_send is not None:before_send()
    observed['stage']='turn_send_claim'
    fence.claim_turn(thread)
    seal_ledger.record('01-send-claimed',{'intent_digest':intent_digest,'threadId':thread})
    result=rpc.request('turn/start',{'threadId':thread,'model':'gpt-6-luna','effort':'xhigh','input':[{'type':'text','text':prompt,'text_elements':[]}]},timeout=RPC_TIMEOUT_SECONDS)
    observed.update(turn_start_result=result,stage='turn_completion_wait')
    turn=result.get('turn',{}).get('id');fence.record_turn(thread,turn)
    completion=rpc.wait_notification('turn/completed',lambda n:n.get('params',{}).get('threadId')==thread and n.get('params',{}).get('turn',{}).get('id')==turn,timeout=COMPLETION_TIMEOUT_SECONDS)
    observed.update(completion_notification=completion,stage='completion_validation')
    finished=completion.get('params',{}).get('turn',{})
    if finished.get('status')!='completed' or finished.get('error') is not None:deny('private turn did not complete cleanly')
    fence.require_read(thread,turn)
    observed['stage']='pre_history_storage_snapshot'
    before=snapshots(cwd)
    observed['stage']='committed_history_read'
    history=exact(rpc.request('private/s05/history.read',{'threadId':thread,'turnId':turn},timeout=RPC_TIMEOUT_SECONDS),'threadId turnId rolloutPath rolloutSha256 prompt output status modelObservations outboundObservations source')
    observed.update(committed_history=history,stage='independent_history_storage_check')
    bound(history,thread,turn)
    if history['source']!='committedOwnedRollout':deny('uncommitted history source')
    raw,after=stable_rollout(history['rolloutPath'],cwd)
    if snapshots(cwd)!=before or before.get(history['rolloutPath'])!=after or after['sha256']!=digest(history['rolloutSha256']):deny('passive history read storage identity changed')
    parsed=parse_rollout(raw,thread,turn,prompt)
    if any(history[k]!=v for k,v in parsed.items()):deny('independent saved rollout differs from committed read')
    observed['stage']='clean_notification_seal'
    trace=rpc.seal_notifications(timeout=SEAL_TIMEOUT_SECONDS)
    observed.update(sealed_trace=trace,stage='sealed_trace_check')
    models,outbounds=trace_check(trace,thread,turn,completion,prompt,parsed['output'],cwd)
    if models!=history['modelObservations'] or outbounds!=history['outboundObservations']:deny('committed telemetry differs from complete live trace')
    final,final_snapshot=stable_rollout(history['rolloutPath'],cwd)
    if parse_rollout(final,thread,turn,prompt)!=parsed:deny('writer-stopped saved rollout semantics changed')
    if stable_rollout(history['rolloutPath'],cwd)[1]!=final_snapshot:deny('final saved rollout is not stable')
    expected=ExpectedPrivateTurn(thread,turn,invocation_key,prompt,cwd)
    normalized={'thread_id':thread,'turn_id':turn,'provider':captured['modelProvider'],'model':captured['model'],'effort':captured['effort'],'approval_policy':captured['approvalPolicy'],'sandbox':'read-only','cwd':captured['cwd'],'ephemeral':captured['ephemeral'],'fallback_enabled':False,'tool_policy_allowed_tools':captured['allowedTools'],'final_tool_specs':outbounds[0]['toolNames']}
    outbound=[{'thread_id':thread,'turn_id':turn,'provider':'openai','model':o['model'],'effort':o['effort'],'tools':o['toolNames']} for o in outbounds]
    serving=[{'thread_id':thread,'turn_id':turn,'response_id':m['responseId'],'model':m['model'],'source':m['source']} for m in models]
    normalized_history=_json({'thread_id':thread,'turn_id':turn,'status':'completed','error':None,'items_view':'full','items':[{'type':'userMessage','text':prompt,'invocation_key':invocation_key},{'type':'agentMessage','text':parsed['output']}]}).encode()
    checked=check_private_evidence(expected,session=normalized,outbound=outbound,serving=serving,history_raw=normalized_history)
    rollout_seal=seal_ledger.seal_rollout(final,expected_sha256=final_snapshot['sha256'])
    seal_ledger.seal_history(normalized_history,expected_sha256=checked.history_sha256)
    observed['stage']='independent_semantic_check'
    semantic=semantic_check(prompt,checked.response_text)
    result={'schema':'jev.s05.private-result/1','thread_id':thread,'turn_id':turn,'response_text':checked.response_text,'captured_session':captured,'session_observation':session_observation,'thread_start_result':started,'model_observations':models,'outbound_observations':outbounds,'complete_notification_trace':trace,'rollout_path':history['rolloutPath'],'committed_rollout_sha256':history['rolloutSha256'],'final_rollout_sha256':final_snapshot['sha256'],'final_rollout_seal':rollout_seal,'saved_readback_sha256':checked.history_sha256,'observation_elapsed_ms':round((time.monotonic()-observation_started)*1000),'rpc_timeout_seconds':RPC_TIMEOUT_SECONDS,'completion_timeout_seconds':COMPLETION_TIMEOUT_SECONDS,'seal_timeout_seconds':SEAL_TIMEOUT_SECONDS,'server_reported_effort':'UNKNOWN','host_resolved_effort':'xhigh','serialized_request_effort':'xhigh','actual_serving_model':'gpt-6-luna','model_evidence_level':checked.model_evidence_level,'internal_backend_model':checked.internal_backend_model,'actual_model_provider':'openai','semantic_correctness':semantic['semantic_correctness'],'semantic_check':semantic}
    seal_ledger.record('03-result',result)
    return result


CHECKER=Path('/Users/tony/Work/Projects/nuanu-ai-lab/artifacts/jev-s05-takeover-20261006/dev-prepare/controls/s05_semantic_case.py')
CHECKER_SHA='71af2e6267430766c1fc24a7dd461a46441e28f8fe76e69c4997ccca6bf19326'
CASE_NONCE='db9aa1f04cdefede75acc20ef5b4fa4e'
def semantic_check(prompt,output):
    # Exact retained source, no alternative evaluator or caller module path.
    from scripts.authenticated_caller_source import _path
    _path(CHECKER)
    raw=CHECKER.read_bytes()
    if hashlib.sha256(raw).hexdigest()!=CHECKER_SHA:deny('retained independent semantic checker identity changed')
    namespace={'__name__':'s05_independent_semantic_checker'}
    exec(compile(raw,str(CHECKER),'exec'),namespace)
    if prompt!=namespace['build_prompt'](CASE_NONCE):deny('prompt is not the exact fresh nonce-bound retained case')
    checked=namespace['check_response'](CASE_NONCE,output.encode())
    if checked['semantic_correctness']!='passed':deny('independent retained semantic case failed')
    return checked
