"""Owned-stdio S05 caller launcher, default deny without private host bootstrap.

No JSON material import, native-agents attestation or semantic-correctness claim.
The returned receipt records configured execution; actual model stays unknown
when the host supplies no serving-model telemetry. Invocation fences are owned
by the existing durable observer ledger and never retried here.

The owner independently provisions TECT_HOST_CONFIG, TECT_SOCKET,
TECT_WORKSPACE_KEY, TECT_NATIVE_SESSION_ID, TECT_CALLER_LEDGER_DIR and
TECT_CALLER_CODEX_EXECUTABLE. The ledger must already be owned/private and the
owner must retain and reuse that same directory across sessions; replacing it
loses replay fences. CLI pins, expected UUIDs, prompt and cwd are untrusted data.
--execute-owned authorizes only the exact accepted current source selection.
Live source/backend acceptance is not established by this offline module.
"""
from __future__ import annotations

import argparse
from dataclasses import asdict, replace
import hashlib
import os
from pathlib import Path
import stat

from scripts.authenticated_caller_source import (AuthenticatedCurrentSource, _HostAdapterContext,
    _host_adapter_context, _path, _decode)
from scripts.caller_host_routing import (CallerHostRouting, CallerRoutingRejected,
    CallerRoutingRequest, _validate_material, _shape, _json)
from scripts.codex_app_server_observer import AppServerObserver, _Ledger, _sha
from scripts.codex_app_server_profile import build_launch_profile
from scripts.codex_app_server_rpc import OwnedAppServerRpc

INSTALLED_EXECUTABLE = "/Applications/Codex.app/Contents/Resources/codex-cli/CodexCLI.app/Contents/MacOS/codex"
# Inventory pin for codex-cli 0.159.0. A legitimate app upgrade requires a fresh
# owner inventory/pin; it never selects a fallback executable or model.
INSTALLED_EXECUTABLE_SHA256 = "ccd1b9441d35ce30102059c78514a125d676e6765ea1d001033e8cbe88718314"


def _private_ledger(directory):
    """Owner provisioned stable directory, retained and reused across invocations."""
    _path(directory)
    node = directory.lstat()
    if not stat.S_ISDIR(node.st_mode) or stat.S_IMODE(node.st_mode) != 0o700 or node.st_uid != os.getuid():
        raise CallerRoutingRejected("fixed host ledger must be existing owned private directory")
    return node.st_dev, node.st_ino


def _host_profile(model, effort, executable):
    if executable != INSTALLED_EXECUTABLE:
        from scripts.private_owned_appserver_profile import private_profile
        return private_profile(model, effort, executable)[0]
    _path(Path(executable))
    node = Path(executable).lstat()
    if not stat.S_ISREG(node.st_mode) or not node.st_mode & 0o111 or node.st_mode & 0o022:
        raise CallerRoutingRejected("privileged App Server executable is not a protected executable file")
    digest = hashlib.sha256()
    with Path(executable).open("rb") as stream:
        opened = os.fstat(stream.fileno())
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(block)
        finished = os.fstat(stream.fileno())
    current = Path(executable).lstat()
    stable = lambda info: (info.st_dev, info.st_ino, info.st_size, info.st_mtime_ns, info.st_ctime_ns, info.st_mode)
    if stable(node) != stable(opened) or stable(opened) != stable(finished) or stable(opened) != stable(current) or digest.hexdigest() != INSTALLED_EXECUTABLE_SHA256:
        raise CallerRoutingRejected("privileged App Server executable inventory pin differs")
    profile = build_launch_profile(model, effort)
    argv = (executable, *profile.argv[1:])
    digest = hashlib.sha256(_json(argv).encode()).hexdigest()
    return replace(profile, argv=argv, digest=digest)


def _deny_observer_reservation(ledger, key_digest):
    """Any historical observer entry burns the key, regardless of readability."""
    try:
        os.stat(key_digest + ".00-reserved.json", dir_fd=ledger.fd, follow_symlinks=False)
    except FileNotFoundError:
        return
    except OSError:
        raise CallerRoutingRejected("observer reservation cannot be checked; no child or retry") from None
    raise CallerRoutingRejected("observer invocation already reserved; no child or retry")


def _executor_route(material):
    selected = material["selected_route"]
    if (selected["provider"] != "openai" or (selected["model"], selected["effort"]) not in {
            ("gpt-6-luna", "xhigh"), ("gpt-6.1-sol", "medium")}):
        raise CallerRoutingRejected("selected route is outside the standing executor allowlist")
    return selected


class _ExecutorSource(AuthenticatedCurrentSource):
    """Check the allowlist again at the bridge-to-execution boundary."""
    def resolve_current(self, request, *, input_sha256):
        snapshot = super().resolve_current(request, input_sha256=input_sha256)
        _executor_route(_validate_material(snapshot, request, input_sha256))
        return snapshot


def _execute_owned(*, host_context: _HostAdapterContext, request, prompt: str,
                   cwd: str, ledger_dir: Path, executable: str):
    """Private owner installation only. Caller pins are untrusted intent data.

    Resolve before starting the child, then freshly resolve at dispatch. Both
    resolutions bind the exact catalogue/work digests and input; no cache is a
    trusted source. Profile construction reads configuration without editing it.
    """
    if not isinstance(prompt, str) or not prompt.strip() or len(prompt.encode("utf-8")) > 16384:
        raise CallerRoutingRejected("bounded exact prompt required")
    if not isinstance(cwd, str) or not Path(cwd).is_absolute() or cwd != cwd.strip():
        raise CallerRoutingRejected("absolute exact execution cwd required")
    if type(request) is not CallerRoutingRequest:
        raise CallerRoutingRejected("exact immutable caller pins required")
    ledger_identity = _private_ledger(ledger_dir)
    key_digest = _sha(request.invocation_key)
    ledger = _Ledger(ledger_dir)
    try:
        _deny_observer_reservation(ledger, key_digest)
    finally:
        ledger.close()
    source = _ExecutorSource(host_context)
    digest = hashlib.sha256(prompt.encode("utf-8")).hexdigest()
    first = source.resolve_current(request, input_sha256=digest)
    selected = _executor_route(_validate_material(first, request, digest))
    profile = _host_profile(selected["model"], selected["effort"], executable)
    if _private_ledger(ledger_dir) != ledger_identity:
        raise CallerRoutingRejected("fixed host ledger changed identity")
    # Durable pre-child fence binds the exact private executable/profile and
    # source snapshot. It only burns an invocation, never authorizes replay.
    ledger = _Ledger(ledger_dir)
    try:
        # Legacy writers can still race this preflight. The observer's atomic
        # reservation remains the provider-send fence across those writers.
        _deny_observer_reservation(ledger, key_digest)
        ledger.write(key_digest + ".00-launch.json",
            {"stage": "owned_launch_reserved", "invocation_key": request.invocation_key,
             "prompt_sha256": digest, "source_binding_digest": first.material_sha256,
             "selected": selected, "executable": executable,
             "executable_sha256": hashlib.sha256(Path(executable).read_bytes()).hexdigest(), "launch_profile_digest": profile.digest})
    except FileExistsError:
        raise CallerRoutingRejected("invocation launch already reserved; no child or retry") from None
    finally:
        ledger.close()
    if executable != INSTALLED_EXECUTABLE:
        return _execute_private_owned(source=source,request=request,prompt=prompt,cwd=cwd,
            ledger_dir=ledger_dir,executable=executable,profile=profile,first=first,
            selected=selected,digest=digest,ledger_identity=ledger_identity)
    with OwnedAppServerRpc(profile.argv, cwd) as rpc:
        rpc.initialize({"name": "s05-authenticated-caller", "version": "1.0.0"})
        if _private_ledger(ledger_dir) != ledger_identity:
            raise CallerRoutingRejected("fixed host ledger changed identity before observation")
        observer = AppServerObserver(rpc, ledger_dir=ledger_dir)
        bridge = CallerHostRouting()
        bridge._install(source, observer)
        return bridge.run_once(request, prompt=prompt, cwd=cwd)


def _safe_failure_reason(error):
    # Only owned static error types may carry a reason, never arbitrary exception text.
    if type(error).__name__ not in {'PrivateS05EvidenceRejected','CallerRoutingRejected','AppServerProtocolError','AppServerRpcError','TimeoutError','EOFError'}:
        return 'UNKNOWN_NONSTATIC_ERROR_REASON'
    import re
    reason=str(error)[:512]
    reason=re.sub(r'(?i)(bearer\s+)[^\s]+',r'\1[REDACTED]',reason)
    reason=re.sub(r'[A-Za-z0-9_./+=-]{40,}','[REDACTED_LONG_TOKEN]',reason)
    return reason


def _finalize_private_failure(*,rpc,ledger_dir,request,intent,source_snapshot,error):
    """Write failure evidence on the consumed invocation, never a PASS receipt."""
    close_error=None
    if rpc is not None:
        try:rpc.close()
        except Exception as caught:close_error=type(caught).__name__
    diagnostics=rpc.retained_private_diagnostics() if rpc is not None else {
        'child_exit_code':None,'stdout_eof_observed':False,'private_wire_frames':[],
        'stdout_notification_trace':[],'clean_notification_seal':False,
        'close_classification':'NO_CHILD_STARTED'}
    observation=getattr(rpc,'_private_s05_observation',{}) if rpc is not None else {}
    notices=diagnostics['stdout_notification_trace']
    models=[n.get('params') for n in notices if n.get('method')=='private/s05/modelObserved']
    outbounds=[n.get('params') for n in notices if n.get('method')=='private/s05/outboundObserved']
    result={'schema':'jev.s05.private-failed-result/1','status':'failed_closed_no_retry',
        'invocation_key':request.invocation_key,'intent':intent,'source_binding_json':source_snapshot.material_json,
        'source_binding_digest':source_snapshot.material_sha256,'failure_stage':observation.get('stage','private_launch_pre_observation'),
        'failure_type':type(error).__name__,'failure_reason':_safe_failure_reason(error),
        'close_error_type':close_error,'partial_observation':observation,'child_diagnostics':diagnostics,
        'model_observations':models,'outbound_observations':outbounds,
        'observed_actual':None,'server_reported_effort':'UNKNOWN','semantic_correctness':'not_assessed',
        'clean_seal_acceptance':False,'invocation_consumed':True,'automatic_retry':False}
    key=_sha(request.invocation_key);result_name=key+'.03-failed-result.json';receipt_name=key+'.04-failed-receipt.json'
    receipt={'schema':'jev.s05.private-failed-receipt/1','status':'failed_closed_no_retry',
        'invocation_key':request.invocation_key,'result_filename':result_name,'result_digest':_sha(_json(result)),
        'intent_digest':None if intent is None else _sha(_json(intent)),
        'source_binding_digest':source_snapshot.material_sha256,'observed_actual':None,
        'clean_seal_acceptance':False,'invocation_consumed':True,'automatic_retry':False}
    ledger=_Ledger(ledger_dir)
    try:
        ledger.write(result_name,result);ledger.write(receipt_name,receipt)
    finally:ledger.close()
    error._private_failure_result=str(ledger_dir/result_name)
    error._private_failure_receipt=str(ledger_dir/receipt_name)


def _execute_private_owned(*,source,request,prompt,cwd,ledger_dir,executable,profile,first,selected,digest,ledger_identity):
    rpc=None;intent=None;source_snapshot=first
    try:
        from scripts.private_s05_adapter import observe_private
        from scripts.private_s05_seal import PrivateSealLedger
        from scripts.private_owned_appserver_profile import private_runtime_home,private_environment
        owned_cwd=private_runtime_home(profile)
        if cwd != owned_cwd:
            raise CallerRoutingRejected('private cwd differs from owner-pinned runtime home')
        # Fresh authenticated normal route preparation; caller JSON is never a
        # source ticket. Same source/task/input bindings must survive dispatch.
        second=source.resolve_current(request,input_sha256=digest)
        source_snapshot=second
        material=_validate_material(second,request,digest)
        if second.material_json != first.material_json or second.material_sha256 != first.material_sha256 or _executor_route(material)!=selected:
            raise CallerRoutingRejected('private current source selection changed before dispatch')
        if _host_profile(selected['model'],selected['effort'],executable)!=profile:
            raise CallerRoutingRejected('private owner profile changed before dispatch')
        intent={'schema':'jev.s05.private-intent/1','invocation_key':request.invocation_key,'prompt_sha256':digest,'cwd':owned_cwd,'ephemeral_thread':False,'selected':selected,'requested':material['requested_route'],'recommended':material['recommended_route'],'source_binding_json':second.material_json,'source_binding_digest':second.material_sha256,'launch_profile_digest':profile.digest,'declared_available_latency_ms':material['preparation']['work']['available_latency_ms']['Known']['value'],'latency_evidence_kind':'route_eligibility_fact_not_measured_SLA','environment':private_environment(profile)}
        intent_digest=_sha(_json(intent))
        seal=PrivateSealLedger(ledger_dir)
        seal.reserve(invocation_key=request.invocation_key,intent_digest=intent_digest,prompt_sha256=digest,source_binding_sha256=second.material_sha256,profile_sha256=profile.digest,intent=intent)
        def before_send():
            current=source.resolve_current(request,input_sha256=digest)
            current_material=_validate_material(current,request,digest)
            if current.material_json!=second.material_json or current.material_sha256!=second.material_sha256 or _executor_route(current_material)!=selected or _private_ledger(ledger_dir)!=ledger_identity or _host_profile(selected['model'],selected['effort'],executable)!=profile:
                raise CallerRoutingRejected('private source owner profile or ledger changed before provider send')
        rpc=OwnedAppServerRpc(profile.argv,owned_cwd,private_stack_environment=private_environment(profile))
        with rpc:
            rpc.initialize({'name':'s05-authenticated-private-caller','version':'1.0.0'},timeout=30.0)
            result=observe_private(rpc,prompt=prompt,cwd=owned_cwd,invocation_key=request.invocation_key,seal_ledger=seal,intent_digest=intent_digest,before_send=before_send)
        receipt={'schema':'jev.s05.private-receipt/1','status':'completed_observed_private_route','evidence_kind':'owned_private_appserver_sealed_saved_readback','invocation_key':request.invocation_key,'intent':intent,'intent_digest':intent_digest,'result':result,'result_digest':_sha(_json(result)),'source_binding_digest':second.material_sha256,'prompt_sha256':digest,'selected':selected,'observed_actual':{'provider':'openai','model':result['actual_serving_model'],'effort':'UNKNOWN'},'host_resolved_effort':'xhigh','serialized_request_effort':'xhigh','semantic_correctness':result['semantic_correctness']}
        seal.record('04-receipt',receipt)
        return receipt
    except Exception as error:
        _finalize_private_failure(rpc=rpc,ledger_dir=ledger_dir,request=request,intent=intent,
            source_snapshot=source_snapshot,error=error)
        raise


def _bootstrap(pins, environment):
    required = ("TECT_HOST_CONFIG", "TECT_SOCKET", "TECT_WORKSPACE_KEY", "TECT_NATIVE_SESSION_ID",
                "TECT_CALLER_LEDGER_DIR", "TECT_CALLER_CODEX_EXECUTABLE")
    if any(not isinstance(environment.get(key), str) or not environment[key] for key in required):
        raise CallerRoutingRejected("required privileged host bootstrap is missing")
    ledger = Path(environment["TECT_CALLER_LEDGER_DIR"])
    _private_ledger(ledger)
    context = _host_adapter_context(socket_path=Path(environment["TECT_SOCKET"]),
        config_path=Path(environment["TECT_HOST_CONFIG"]), workspace_key=environment["TECT_WORKSPACE_KEY"],
        native_session_id=environment["TECT_NATIVE_SESSION_ID"], workspace_id=pins["expected_workspace_id"],
        actor_id=pins["expected_actor_id"], session_id=pins["expected_session_id"])
    return context, ledger, environment["TECT_CALLER_CODEX_EXECUTABLE"]


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--execute-owned", action="store_true", help="owner authorizes exactly the accepted source selection")
    parser.add_argument("--pins-json", help="bounded data-only source request and expected identity pins; never authorization")
    parser.add_argument("--prompt-file", help="bounded exact UTF-8 task input")
    parser.add_argument("--cwd")
    args = parser.parse_args(argv)
    try:
        if not args.execute_owned:
            print(_json({"status": "execution_denied_default", "observed_actual": None}))
            return 1
        if not isinstance(args.pins_json, str) or len(args.pins_json.encode()) > 4096:
            raise CallerRoutingRejected("bounded data-only source pins required")
        fields = " ".join(CallerRoutingRequest.__dataclass_fields__) + " expected_workspace_id expected_actor_id expected_session_id"
        pins = _shape(_decode(args.pins_json.encode()), fields)
        request = CallerRoutingRequest(**{field: pins[field] for field in CallerRoutingRequest.__dataclass_fields__})
        context, ledger, executable = _bootstrap(pins, os.environ)
        if not args.prompt_file:
            raise CallerRoutingRejected("bounded exact prompt file required")
        with Path(args.prompt_file).open("rb") as stream:
            raw = stream.read(16385)
        if len(raw) > 16384:
            raise CallerRoutingRejected("task input exceeds UTF-8 bound")
        prompt = raw.decode("utf-8")
        receipt = _execute_owned(host_context=context, request=request, prompt=prompt,
            cwd=args.cwd, ledger_dir=ledger, executable=executable)
        if isinstance(receipt,dict):
            print(_json(receipt))
            return 0 if receipt['status']=='completed_observed_private_route' else 1
        print(_json({"status": receipt.status, "evidence_kind": receipt.evidence_kind,
            "invocation_key": receipt.invocation_key, "prompt_sha256": receipt.prompt_digest,
            "source_binding_digest": receipt.source_binding_digest, "selected": asdict(receipt.selection),
            "observed_actual": receipt.observed_actual, "semantic_correctness": "not_assessed"}))
        return 0 if receipt.status == "completed_configured_route" else 1
    except Exception as error:
        failure={"status":"execution_denied_without_retry","error_type":type(error).__name__,"observed_actual":None}
        if hasattr(error,'_private_failure_result'):
            failure.update(failure_reason=_safe_failure_reason(error),failure_result=error._private_failure_result,
                failure_receipt=error._private_failure_receipt)
        print(_json(failure))
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
