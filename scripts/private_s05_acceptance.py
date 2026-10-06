"""Independent retained-evidence evaluator. No host call or authentication claim."""
import argparse,hashlib,json,os,stat
from pathlib import Path
from scripts.caller_host_routing import _json
from scripts.codex_app_server_observer import _sha
from scripts.private_s05_adapter import session_projection,telemetry,trace_check,semantic_check,deny
from scripts.private_s05_rollout import parse_rollout,decode

def read_owned(path):
    from scripts.authenticated_caller_source import _path
    _path(path);n=path.lstat()
    if not stat.S_ISREG(n.st_mode) or n.st_uid!=os.getuid() or n.st_mode&0o077 or n.st_nlink!=1 or n.st_size>16*1024*1024:deny('retained evidence is not bounded private regular storage')
    raw=path.read_bytes();after=path.lstat()
    identity=lambda x:(x.st_dev,x.st_ino,x.st_size,x.st_mtime_ns,x.st_ctime_ns)
    if identity(n)!=identity(after):deny('retained evidence changed during read')
    return raw

def evaluate(ledger,invocation_key):
    ledger=Path(ledger);key=_sha(invocation_key)
    receipt=decode(read_owned(ledger/(key+'.04-receipt.json')));reserved=decode(read_owned(ledger/(key+'.00-reserved.json')));send=decode(read_owned(ledger/(key+'.01-send-claimed.json')));retained_result=decode(read_owned(ledger/(key+'.03-result.json')))
    if receipt.get('schema')!='jev.s05.private-receipt/1' or receipt.get('status')!='completed_observed_private_route' or receipt.get('invocation_key')!=invocation_key:deny('invalid private retained receipt')
    intent=receipt['intent'];result=receipt['result']
    if intent!=reserved.get('intent') or receipt['intent_digest']!=_sha(_json(intent)) or reserved['intent_digest']!=receipt['intent_digest'] or result!=retained_result or receipt['result_digest']!=_sha(_json(result)):deny('retained intent result or receipt digest mismatch')
    if intent['invocation_key']!=invocation_key or intent['source_binding_digest']!=_sha(intent['source_binding_json']) or reserved['source_binding_sha256']!=intent['source_binding_digest'] or reserved['profile_sha256']!=intent['launch_profile_digest']:deny('retained source or private profile binding differs')
    thread=result['thread_id'];turn=result['turn_id']
    if send.get('threadId')!=thread or send.get('intent_digest')!=receipt['intent_digest']:deny('provider send fence differs')
    normalized_raw=read_owned(ledger/(key+'.02-history.json'));history=decode(normalized_raw)
    if hashlib.sha256(normalized_raw).hexdigest()!=result['saved_readback_sha256'] or history.get('thread_id')!=thread or history.get('turn_id')!=turn:deny('normalized saved seal differs')
    items=history.get('items');users=[i for i in items if i.get('type')=='userMessage'];agents=[i for i in items if i.get('type')=='agentMessage']
    if len(users)!=1 or len(agents)!=1 or users[0].get('invocation_key')!=invocation_key:deny('saved turn prompt/output cardinality differs')
    prompt=users[0]['text'];output=agents[0]['text']
    if _sha(prompt)!=intent['prompt_sha256'] or reserved['prompt_sha256']!=intent['prompt_sha256'] or result['response_text']!=output:deny('exact prompt response binding differs')
    raw=read_owned(ledger/(key+'.02-rollout.jsonl'))
    if hashlib.sha256(raw).hexdigest()!=result['final_rollout_sha256'] or result['final_rollout_seal']!=key+'.02-rollout.jsonl':deny('actual raw rollout seal differs')
    parsed=parse_rollout(raw,thread,turn,prompt)
    if parsed['output']!=output or parsed['modelObservations']!=result['model_observations'] or parsed['outboundObservations']!=result['outbound_observations']:deny('independent raw storage disagrees with retained telemetry or response')
    captured=session_projection(result['session_observation'],thread,intent['cwd'])
    if captured!=result['captured_session']:deny('actual Session projection differs')
    telemetry(result['model_observations'],result['outbound_observations'],thread,turn)
    completions=[n for n in result['complete_notification_trace'] if n.get('method')=='turn/completed']
    if len(completions)!=1:deny('incomplete retained notification seal')
    models,outbounds=trace_check(result['complete_notification_trace'],thread,turn,completions[0],prompt,output,intent['cwd'])
    if models!=parsed['modelObservations'] or outbounds!=parsed['outboundObservations']:deny('saved raw telemetry differs from complete trace')
    if result.get('model_evidence_level')!='provider_declared_used_model_id' or result.get('internal_backend_model')!='UNKNOWN':deny('model proof level differs from provider declared used ID')
    semantic=semantic_check(prompt,output)
    if semantic!=result['semantic_check'] or result['server_reported_effort']!='UNKNOWN' or receipt['observed_actual']!={'provider':'openai','model':'gpt-6-luna','effort':'UNKNOWN'}:deny('semantic result or effort proof levels differ')
    return {'schema':'jev.s05.private-retained-acceptance/1','status':'passed_retained_checks','provenance':'offline_owner_retained_evidence_not_new_authentication','invocation_key':invocation_key,'semantic_correctness':'passed','saved_raw_rollout_sha256':result['final_rollout_sha256'],'actual_serving_model':'gpt-6-luna','model_evidence_level':'provider_declared_used_model_id','internal_backend_model':'UNKNOWN','provider_model_metadata_sources':sorted({m['source'] for m in models}),'effective_policy_allowed_tools':captured['allowedTools'],'final_serialized_tools':outbounds[0]['toolNames'],'host_resolved_effort':captured['effort'],'serialized_request_effort':outbounds[0]['effort'],'server_reported_effort':'UNKNOWN','declared_available_latency_ms':intent['declared_available_latency_ms'],'observation_elapsed_ms':result['observation_elapsed_ms'],'latency_assessment':'not_an_SLA_assertion'}

def main(argv=None):
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--ledger-dir',required=True);parser.add_argument('--invocation-key',required=True);args=parser.parse_args(argv)
    try:print(_json(evaluate(args.ledger_dir,args.invocation_key)));return 0
    except Exception as e:print(_json({'status':'retained_checks_failed','error_type':type(e).__name__}));return 1
if __name__=='__main__':raise SystemExit(main())
