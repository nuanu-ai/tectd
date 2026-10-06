"""Separate owner-pinned no-turn readiness. Explicit execution; no acceptance proof."""
import argparse,hashlib,json,time,os,stat,re
from pathlib import Path
from scripts.private_owned_appserver_profile import READINESS_INVENTORY,private_profile,private_runtime_home,private_environment
from scripts.private_s05_adapter import session_projection,check_disabled_startup_notice,check_configuration_notice,RPC_TIMEOUT_SECONDS,SEAL_TIMEOUT_SECONDS,deny
from scripts.private_s05_seal import OneFreshPrivateTurn
from scripts.codex_app_server_rpc import OwnedAppServerRpc
from scripts.codex_app_server_observer import _Ledger
from scripts.bounded_caller_route_launcher import _private_ledger
from scripts.caller_host_routing import _json
READINESS_LEDGER=READINESS_INVENTORY.parent.parent/'readiness-ledger-07'

def inspect_readiness(rpc,home):
    rpc._readiness_wire_trace=[]
    initialized=rpc.initialize({'name':'s05-owner-no-turn-readiness','version':'1.0.0'},timeout=RPC_TIMEOUT_SECONDS)
    rpc._readiness_wire_trace.append({'method':'initialize','result':initialized})
    def query(method,params):
        response=rpc.request(method,params,timeout=RPC_TIMEOUT_SECONDS)
        rpc._readiness_wire_trace.append({'method':method,'params':params,'result':response})
        return response
    catalogue=[];cursor=None
    for page in range(4):
        params={'limit':100,'includeHidden':True}
        if cursor is not None:params['cursor']=cursor
        result=query('model/list',params)
        if not isinstance(result.get('data'),list) or len(result['data'])>100:deny('unbounded readiness model catalogue')
        catalogue.extend(result['data']);cursor=result.get('nextCursor')
        if cursor is None:break
        if not isinstance(cursor,str) or not cursor or len(cursor)>1024:deny('invalid readiness model cursor')
    else:deny('readiness model catalogue exceeds four pages')
    matches=[row for row in catalogue if isinstance(row,dict) and row.get('model')=='gpt-6-luna']
    if len(matches)!=1 or not isinstance(matches[0].get('supportedReasoningEfforts'),list) or not any(isinstance(e,dict) and e.get('reasoningEffort')=='xhigh' for e in matches[0]['supportedReasoningEfforts']):deny('catalogue does not advertise exact Luna/xhigh')
    fence=OneFreshPrivateTurn();fence.claim_start()
    started_thread=query('thread/start',{'model':'gpt-6-luna','modelProvider':'openai','ephemeral':False,'allowProviderModelFallback':False})
    thread=started_thread.get('thread',{}).get('id');fence.record_thread(thread)
    session_observation=query('private/s05/session.read',{'threadId':thread})
    captured=session_projection(session_observation,thread,home)
    trace=rpc.seal_notifications(timeout=SEAL_TIMEOUT_SECONDS)
    startup_phase=True;remote_seen=False;configuration_seen=set()
    for notice in trace:
        method=notice.get('method');p=notice.get('params',{})
        if method=='remoteControl/status/changed':
            remote_seen=check_disabled_startup_notice(notice,startup_phase=startup_phase,already_seen=remote_seen)
            continue
        if method in {'deprecationNotice','warning'}:
            check_configuration_notice(notice,thread=thread,cwd=home,pre_turn=True,seen=configuration_seen)
            continue
        startup_phase=False
        if method=='thread/started':
            if p.get('thread',{}).get('id')!=thread:deny('foreign readiness thread')
        elif method=='thread/status/changed':
            if p.get('threadId')!=thread or p.get('status')!={'type':'idle'}:deny('active or foreign readiness thread')
        else:deny('readiness emitted turn tool outbound or unexpected notification')
    return {'schema':'jev.s05.no-turn-readiness/1','status':'captured_no_turn_readiness','threadId':thread,'catalogue_model':matches[0],'catalogue_source':'model/list','catalogue_fetch_provenance':'UNKNOWN_cached_bundled_or_remote_not_exposed_by_wire','catalogue_source_policy':'OnlineIfUncached','thread_start_result':started_thread,'session_observation':session_observation,'captured_session':captured,'session_source':'capturedSessionConfigurationAndToolPolicy','notification_trace':trace,'notification_capture_complete':True,'provider_reported_serving_model':'UNKNOWN','serialized_request_effort':'UNKNOWN','server_reported_effort':'UNKNOWN','final_serialized_tools':'UNKNOWN','inference_turn_sent':False,'semantic_correctness':'not_assessed'}

def source_identity(home):
    # Metadata-only identity retained; raw credentials never enter evidence.
    result={}
    for name in ('config.toml','auth.json'):
        path=Path(home)/name;before=path.lstat()
        if path.is_symlink() or not stat.S_ISREG(before.st_mode) or before.st_uid!=os.getuid() or before.st_mode&0o077:deny('native auth source is not owned private regular storage')
        raw=path.read_bytes();after=path.lstat()
        identity=lambda n:(n.st_dev,n.st_ino,n.st_size,n.st_mtime_ns,n.st_ctime_ns,n.st_mode,n.st_uid)
        if identity(before)!=identity(after):deny('native auth source changed during identity read')
        result[name]={'identity':identity(after),'sha256':hashlib.sha256(raw).hexdigest()}
    return result

def execute_probe(command,home,*,private_stack_environment,rpc_factory=OwnedAppServerRpc):
    """One child only, diagnostic retention after close, never retry or turn."""
    started=time.monotonic();rpc=None;error=None;result=None
    try:
        if type(private_stack_environment) is not dict or private_stack_environment!={'RUST_MIN_STACK':'8388608'}:deny('exact private stack-only child environment required')
        rpc=rpc_factory(command,home,private_stack_environment=private_stack_environment)
        result=inspect_readiness(rpc,home)
    except Exception as caught:
        error=caught
    finally:
        if rpc is not None:
            try:rpc.close()
            except Exception as close_error:
                if error is None:error=close_error
    # Passive diagnostics are read only after the child has been closed.
    retained=rpc.retained_private_diagnostics() if rpc is not None else {}
    raw=bytes(getattr(rpc,'_stderr_tail',b'')) if rpc is not None else b''
    # Credentials may occur in an unexpected child error. Retain bounded redacted
    # text, raw-byte digest and exit status, never raw auth/bearer/JWT/key material.
    text=raw.decode('utf-8',errors='replace')
    text=re.sub(r'(?i)(bearer\s+)[^\s]+',r'\1[REDACTED]',text)
    text=re.sub(r'[A-Za-z0-9_./+=-]{40,}', '[REDACTED_LONG_TOKEN]',text)
    text=re.sub(r'(?i)(access_token|refresh_token|id_token|api_key|authorization)([\s:=]+)[^\s,}]+',r'\1\2[REDACTED]',text)
    diagnostics={'child_exit_code':getattr(getattr(rpc,'_process',None),'returncode',None),'stderr_redacted':text,'stderr_raw_sha256':hashlib.sha256(raw).hexdigest(),'stderr_retained_bytes':len(raw),'monotonic_elapsed_ms':round((time.monotonic()-started)*1000),'error_type':None if error is None else type(error).__name__,'error_method':getattr(error,'method',None),'error_code':getattr(error,'code',None)}
    diagnostics.update(retained)
    if result is None:result={'schema':'jev.s05.no-turn-readiness/1','status':'failed_closed_no_retry','inference_turn_sent':False,'serving_model':'UNKNOWN','provider_reported_serving_model':'UNKNOWN','serialized_request_effort':'UNKNOWN','server_reported_effort':'UNKNOWN','final_serialized_tools':'UNKNOWN','usage':'UNKNOWN','semantic_correctness':'not_assessed'}
    result['metadata_rpc_trace']=list(getattr(rpc,'_readiness_wire_trace',[])) if rpc is not None else []
    result['stdout_notification_trace']=list(getattr(rpc,'_all_notifications',[])) if rpc is not None else []
    result['private_rpc_diagnostics']=retained
    result['stdout_eof_observed']=bool(getattr(rpc,'_stdout_eof',False))
    result['notification_capture_complete']=error is None
    result['child_diagnostics']=diagnostics
    return result,error

def main(argv=None):
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--execute-owned-readiness',action='store_true');args=parser.parse_args(argv)
    if not args.execute_owned_readiness:print(_json({'status':'readiness_denied_default'}));return 1
    try:
        # Fixed inventory only; the supplied executable is read from owner data,
        # then independently verified by the frozen profile checker before launch.
        data=json.loads(READINESS_INVENTORY.read_bytes());executable=data['executable']
        profile,_=private_profile('gpt-6-luna','xhigh',executable,purpose='readiness');home=private_runtime_home(profile)
        from scripts.private_owned_appserver_profile import INVENTORY
        if INVENTORY.exists() and json.loads(INVENTORY.read_bytes()).get('private_runtime_home')==home:deny('readiness home must differ from ordinary run home')
        if data['readonly_auth_source_backend']!='file':deny('current readiness requires measured file source identity')
        before=source_identity(data['readonly_auth_source_home']);started=time.monotonic()
        _private_ledger(READINESS_LEDGER);ledger=_Ledger(READINESS_LEDGER)
        try:
            intent={'schema':'jev.s05.readiness-intent/1','profile_sha256':profile.digest,'runtime_home':home,'turn_authorized':False,'auth_source_identity_before':before,'environment':private_environment(profile)}
            ledger.write('readiness.00-intent.json',intent)
            result,error=execute_probe(profile.argv,home,private_stack_environment=private_environment(profile))
            try:after=source_identity(data['readonly_auth_source_home'])
            except Exception as identity_error:
                after={'identity_observation':'UNKNOWN','error_type':type(identity_error).__name__}
                if error is None:error=identity_error
            result.update(monotonic_elapsed_ms=round((time.monotonic()-started)*1000),auth_source_identity_before=before,auth_source_identity_after=after,auth_source_identity_unchanged=before==after,rpc_timeout_seconds=RPC_TIMEOUT_SECONDS,seal_timeout_seconds=SEAL_TIMEOUT_SECONDS,profile_sha256=profile.digest,runtime_home=home)
            ledger.write('readiness.01-result.json',result)
            receipt={'schema':'jev.s05.readiness-receipt/1','intent_sha256':hashlib.sha256(_json(intent).encode()).hexdigest(),'result_sha256':hashlib.sha256(_json(result).encode()).hexdigest(),'status':result['status'],'inference_turn_sent':False,'auth_source_identity_unchanged':before==after}
            ledger.write('readiness.02-receipt.json',receipt)
            if before!=after:deny('native source identity changed during no-turn readiness')
            if error is not None:raise error
        finally:ledger.close()
        print(_json(result));return 0
    except Exception as error:
        print(_json({'status':'readiness_denied_without_retry','error_type':type(error).__name__,'error_method':getattr(error,'method',None),'error_code':getattr(error,'code',None)}));return 1
if __name__=='__main__':raise SystemExit(main())
