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
# Inventory pin for codex-cli 0.159.2. A legitimate app upgrade requires a fresh
# owner inventory/pin; it never selects a fallback executable or model.
INSTALLED_EXECUTABLE_SHA256 = "50ac633af64851511f9bbc71032cdae7f1ba20b3234c189687d61ba846c354c5"


def _private_ledger(directory):
    """Owner provisioned stable directory, retained and reused across invocations."""
    _path(directory)
    node = directory.lstat()
    if not stat.S_ISDIR(node.st_mode) or stat.S_IMODE(node.st_mode) != 0o700 or node.st_uid != os.getuid():
        raise CallerRoutingRejected("fixed host ledger must be existing owned private directory")
    return node.st_dev, node.st_ino


def _host_profile(model, effort, executable):
    if executable != INSTALLED_EXECUTABLE:
        raise CallerRoutingRejected("privileged host executable is not the supported installed App Server")
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
             "executable_sha256": INSTALLED_EXECUTABLE_SHA256, "launch_profile_digest": profile.digest})
    except FileExistsError:
        raise CallerRoutingRejected("invocation launch already reserved; no child or retry") from None
    finally:
        ledger.close()
    with OwnedAppServerRpc(profile.argv, cwd) as rpc:
        rpc.initialize({"name": "s05-authenticated-caller", "version": "1.0.0"})
        if _private_ledger(ledger_dir) != ledger_identity:
            raise CallerRoutingRejected("fixed host ledger changed identity before observation")
        observer = AppServerObserver(rpc, ledger_dir=ledger_dir)
        bridge = CallerHostRouting()
        bridge._install(source, observer)
        return bridge.run_once(request, prompt=prompt, cwd=cwd)


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
        print(_json({"status": receipt.status, "evidence_kind": receipt.evidence_kind,
            "invocation_key": receipt.invocation_key, "prompt_sha256": receipt.prompt_digest,
            "source_binding_digest": receipt.source_binding_digest, "selected": asdict(receipt.selection),
            "observed_actual": receipt.observed_actual, "semantic_correctness": "not_assessed"}))
        return 0 if receipt.status == "completed_configured_route" else 1
    except Exception as error:
        print(_json({"status": "execution_denied_without_retry", "error_type": type(error).__name__,
                     "observed_actual": None}))
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
