"""Pure S05 private evidence checks; no RPC, launch, auth, files or fallback.

Inputs are normalized by a separately reviewed host-wire adapter. These values
are evidence only when the adapter obtained them from its owned host process;
JSON supplied by a caller is never a trusted composition input.
"""
from __future__ import annotations
from dataclasses import dataclass
import hashlib
import json
from scripts.authenticated_caller_source import _decode
from scripts.caller_host_routing import CallerRoutingRejected, _json

class PrivateS05EvidenceRejected(CallerRoutingRejected):
    pass

@dataclass(frozen=True)
class ExpectedPrivateTurn:
    thread_id: str
    turn_id: str
    invocation_key: str
    prompt: str
    cwd: str

@dataclass(frozen=True)
class CheckedPrivateEvidence:
    history_sha256: str
    prompt_sha256: str
    response_text: str
    serving_model: str
    serving_model_sources: tuple[str, ...]
    model_evidence_level: str = 'provider_declared_used_model_id'
    internal_backend_model: str = 'UNKNOWN'
    configured_provider: str = 'openai'
    configured_model: str = 'gpt-6-luna'
    configured_effort: str = 'xhigh'
    serialized_request_effort: str = 'xhigh'
    server_reported_effort: None = None
    tools_empty_observed: bool = True


def _deny(message):
    raise PrivateS05EvidenceRejected(message)


def _bound(record, expected):
    if not isinstance(record,dict) or (record.get('thread_id'),record.get('turn_id')) != (expected.thread_id,expected.turn_id):
        _deny('private evidence belongs to a different thread or turn')


def check_private_evidence(expected, *, session, outbound, serving, history_raw):
    """Check Session, outbound and provider-declared used model ID with saved history.

    The normalized Session must be captured after final turn tool resolution.
    Server effort remains UNKNOWN even when host and outbound effort are xhigh.
    """
    if type(expected) is not ExpectedPrivateTurn:
        _deny('exact expected private turn required')
    _bound(session,expected)
    exact={'provider':'openai','model':'gpt-6-luna','effort':'xhigh','approval_policy':'never','sandbox':'read-only','cwd':expected.cwd,'ephemeral':False,'fallback_enabled':False}
    if any(type(session.get(k)) is not type(v) or session.get(k) != v for k,v in exact.items()):
        _deny('captured private Session differs from the authorized execution boundary')
    if session.get('tool_policy_allowed_tools') != [] or session.get('final_tool_specs') != []:
        _deny('captured no-tools policy and final tool specs must both be empty')
    if not isinstance(outbound,list) or len(outbound) != 1:
        _deny('exactly one serialized outbound inference request required')
    request=outbound[0];_bound(request,expected)
    if (request.get('provider'),request.get('model'),request.get('effort'),request.get('tools')) != ('openai','gpt-6-luna','xhigh',[]):
        _deny('serialized outbound request differs from authorized model effort or tools')
    response_id=request.get('response_id')
    if response_id is not None and (not isinstance(response_id,str) or not response_id):
        _deny('invalid optional provider response binding')
    if not isinstance(serving,list) or not serving:
        _deny('provider-declared used model metadata is absent')
    sources=[];known_response_ids=set()
    for observation in serving:
        _bound(observation,expected)
        observed_id=observation.get('response_id')
        if observed_id is not None and (not isinstance(observed_id,str) or not observed_id.strip() or observed_id!=observed_id.strip()):
            _deny('invalid provider response identity')
        if observation.get('source')=='remoteResponseObjectModel' and observed_id is None:
            _deny('completed remote response object requires actual response identity')
        if observed_id is not None:known_response_ids.add(observed_id)
        if len(known_response_ids)>1:_deny('provider model metadata contains conflicting response identities')
        if response_id is not None and observed_id != response_id:
            _deny('provider model metadata belongs to another response')
        if observation.get('model') != 'gpt-6-luna':
            _deny('provider model metadata conflicts with the selected model')
        if observation.get('source') not in {'remoteResponseObjectModel','http.openai-model','sse.response.headers.openai-model','httpOpenAIModelHeader','websocketHandshakeOpenAIModelHeader','sseResponseHeaders','sseTopLevelHeaders','websocketResponseHeaders','websocketTopLevelHeaders'}:
            _deny('model ID requires provider protocol metadata provenance')
        sources.append(observation['source'])
    if not isinstance(history_raw,bytes) or len(history_raw)>262144:
        _deny('bounded saved history bytes required')
    try:history=_decode(history_raw)
    except Exception:_deny('saved history is malformed')
    _bound(history,expected)
    if history.get('status') != 'completed' or history.get('error') is not None or history.get('items_view') != 'full':
        _deny('full completed saved turn required')
    items=history.get('items')
    if not isinstance(items,list) or any(not isinstance(x,dict) or x.get('type') not in {'userMessage','agentMessage','reasoning'} for x in items):
        _deny('saved history contains tools or unknown items')
    users=[x for x in items if x['type']=='userMessage']
    if len(users)!=1 or users[0].get('text')!=expected.prompt or users[0].get('invocation_key')!=expected.invocation_key:
        _deny('saved prompt or invocation identity differs')
    agents=[x.get('text') for x in items if x['type']=='agentMessage']
    if len(agents)!=1 or not isinstance(agents[0],str) or not agents[0].strip():
        _deny('one completed saved assistant response required')
    return CheckedPrivateEvidence(hashlib.sha256(history_raw).hexdigest(),hashlib.sha256(expected.prompt.encode()).hexdigest(),agents[0],'gpt-6-luna',tuple(sorted(set(sources))))


def compare_passive_read(before, after, *, expected_history_sha256):
    """Compare independent storage snapshots; a host-declared passive flag is insufficient."""
    if not isinstance(before,dict) or before != after:
        _deny('private history read changed its independently observed storage snapshot')
    if before.get('history_sha256') != expected_history_sha256:
        _deny('private saved-history snapshot differs from checked readback')
