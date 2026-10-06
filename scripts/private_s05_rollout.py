"""Independent passive reader for the final writer-stopped owned JSONL rollout."""
import hashlib,json,os,stat
from pathlib import Path
from scripts.authenticated_caller_source import _path
from scripts.private_s05_evidence import PrivateS05EvidenceRejected
from scripts.caller_host_routing import _json
MAX_ROLLOUT=16*1024*1024
# Provider protocol metadata, including completed response.model; no internal-backend attestation.
MODEL_SOURCES={'remoteResponseObjectModel','httpOpenAIModelHeader','websocketHandshakeOpenAIModelHeader','sseResponseHeaders','sseTopLevelHeaders','websocketResponseHeaders','websocketTopLevelHeaders'}

def deny(message):raise PrivateS05EvidenceRejected(message)
def decode(raw):
    def unique(pairs):
        d={}
        for k,v in pairs:
            if k in d:deny('duplicate private rollout field')
            d[k]=v
        return d
    try:return json.loads(raw,object_pairs_hook=unique,parse_constant=lambda _:deny('nonfinite private rollout number'))
    except PrivateS05EvidenceRejected:raise
    except Exception:deny('malformed private rollout JSON')

def stable_rollout(path,home):
    path=Path(path);home=Path(home);_path(path);_path(home)
    if not path.is_relative_to(home) or not path.name.endswith('.jsonl'):deny('rollout is outside the owned runtime home')
    for p in [path,*[x for x in path.parents if x==home or x.is_relative_to(home)]]:
        n=p.lstat()
        if n.st_uid!=os.getuid() or n.st_mode&0o077 or (p==path and (not stat.S_ISREG(n.st_mode) or n.st_nlink!=1)):deny('rollout path is not private owned regular storage')
    def ident(n):return (n.st_dev,n.st_ino,n.st_size,n.st_mtime_ns,n.st_ctime_ns,n.st_mode,n.st_uid,n.st_nlink)
    before=path.lstat()
    with path.open('rb') as f:
        opened=os.fstat(f.fileno());raw=f.read(MAX_ROLLOUT+1);finished=os.fstat(f.fileno())
    after=path.lstat()
    if len(raw)>MAX_ROLLOUT or not ident(before)==ident(opened)==ident(finished)==ident(after):deny('rollout changed during independent read')
    return raw,{'identity':ident(after),'sha256':hashlib.sha256(raw).hexdigest()}

def parse_rollout(raw,thread_id,turn_id,prompt):
    models=[];outbounds=[];starts=ends=users=0;output=None;meta=0;active=False
    if not isinstance(raw,bytes) or not raw.endswith(b'\n'):deny('complete rollout JSONL required')
    for line in raw.splitlines():
        row=decode(line)
        if not isinstance(row,dict) or not isinstance(row.get('payload'),dict):deny('invalid rollout envelope')
        kind=row.get('type');p=row['payload']
        if kind=='session_meta':
            meta+=1
            if p.get('id')!=thread_id:deny('foreign session rollout')
        elif kind=='turn_context':
            if p.get('turn_id')!=turn_id:deny('foreign turn context')
        elif kind=='response_item':
            if p.get('type') not in {'message','agent_message','reasoning'}:deny('tool or unknown response item in saved rollout')
            if p.get('type')=='agent_message':
                content=p.get('content')
                if p.get('author')!='assistant' or p.get('recipient')!='all' or not isinstance(content,list) or any(not isinstance(c,dict) or c.get('type')!='input_text' for c in content):deny('unknown or tool-addressed agent message in saved rollout')
            if p.get('type')=='message':
                role=p.get('role');content=p.get('content')
                if role not in {'user','assistant','developer','system'} or not isinstance(content,list) or any(not isinstance(c,dict) or c.get('type') not in {'input_text','output_text'} for c in content):deny('unknown message content in saved rollout')
        elif kind=='event_msg':
            event=p.get('type')
            if event in {'task_started','turn_started'}:
                starts+=1;active=True
                if p.get('turn_id')!=turn_id or starts!=1:deny('second or foreign saved turn')
            elif event=='user_message':
                users+=1
                if not active or p.get('message')!=prompt or users!=1:deny('saved prompt differs')
            elif event=='thread_settings_applied':
                settings=p.get('thread_settings',{})
                if p.get('thread_id')!=thread_id or not isinstance(settings,dict) or any(settings.get(k)!=v for k,v in {'model':'gpt-6-luna','model_provider_id':'openai','reasoning_effort':'xhigh','approval_policy':'never'}.items()):deny('saved thread settings conflict with owned profile')
            elif event=='agent_message':
                if not active or not isinstance(p.get('message'),str):deny('invalid saved assistant output')
                output=p['message']
            elif event=='private_s05_model_observed':
                if p.get('turn_id')!=turn_id:deny('foreign saved model observation')
                models.append({'threadId':thread_id,'turnId':turn_id,'responseId':p.get('response_id'),'model':p.get('model'),'source':p.get('source')})
            elif event=='private_s05_outbound_observed':
                if p.get('turn_id')!=turn_id:deny('foreign saved outbound observation')
                outbounds.append({'threadId':thread_id,'turnId':turn_id,'model':p.get('model'),'effort':p.get('effort'),'toolNames':p.get('tool_names'),'toolsSha256':p.get('tools_sha256'),'requestSha256':p.get('request_sha256'),'source':p.get('source')})
            elif event in {'task_complete','turn_complete'}:
                ends+=1
                if not active or p.get('turn_id')!=turn_id or p.get('error') is not None or p.get('last_agent_message')!=output or ends!=1:deny('invalid saved turn completion')
                active=False
            elif event not in {'token_count','agent_reasoning','agent_reasoning_raw_content','agent_reasoning_section_break','agent_message_delta','agent_reasoning_delta','agent_reasoning_raw_content_delta'}:deny('tool reroute or unknown event in saved rollout')
        elif kind not in {'token_usage_record','world_state','security_risk_score'}:deny('unknown owned rollout record')
    if meta!=1 or starts!=1 or ends!=1 or users!=1 or active or not output:deny('full single completed owned rollout required')
    return {'threadId':thread_id,'turnId':turn_id,'prompt':prompt,'output':output,'status':'completed','modelObservations':models,'outboundObservations':outbounds}
