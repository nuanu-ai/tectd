"""Session-scoped owner inventory, separate from the stock installed host class.

Caller pins cannot select this inventory or provide executable/profile authority.
The root must freeze it only after reviewing the private build/startup receipt.
"""
from __future__ import annotations
import hashlib
import json
import os
from pathlib import Path
import stat
from dataclasses import dataclass
from scripts.codex_app_server_profile import AppServerLaunchProfile
from scripts.caller_host_routing import CallerRoutingRejected, _json
from scripts.authenticated_caller_source import _path, _decode

INVENTORY = Path('/Users/tony/Work/Projects/nuanu-ai-lab/artifacts/jev-s05-takeover-20261006/dev-prepare/config/private-host-ordinary-03.json')
READINESS_INVENTORY=INVENTORY.with_name('private-host-readiness-07.json')
SCHEMA = 'jev.s05.owner-private-appserver-inventory/1'
PRIVATE_STACK_ENV={'RUST_MIN_STACK':'8388608'}

@dataclass(frozen=True)
class PrivateOwnedLaunchProfile(AppServerLaunchProfile):
    environment: tuple[tuple[str,str],...]

def private_environment(profile):
    if type(profile) is not PrivateOwnedLaunchProfile or profile.environment!=tuple(PRIVATE_STACK_ENV.items()):
        raise CallerRoutingRejected('exact private stack-only environment profile required')
    return dict(profile.environment)


def _stable(path: Path, *, executable=False):
    _path(path)
    before = path.lstat()
    mode = stat.S_IMODE(before.st_mode)
    if (not stat.S_ISREG(before.st_mode) or before.st_uid != os.getuid()
            or mode & 0o222 or (executable and not mode & 0o111)):
        raise CallerRoutingRejected('private host inventory and binary must be owner-frozen files')
    h = hashlib.sha256()
    with path.open('rb') as stream:
        opened = os.fstat(stream.fileno())
        for block in iter(lambda: stream.read(1024 * 1024), b''):
            h.update(block)
        finished = os.fstat(stream.fileno())
    after = path.lstat()
    identity = lambda s: (s.st_dev,s.st_ino,s.st_size,s.st_mtime_ns,s.st_ctime_ns,s.st_mode,s.st_uid)
    if not identity(before) == identity(opened) == identity(finished) == identity(after):
        raise CallerRoutingRejected('private host frozen file changed identity')
    return h.hexdigest()


def _private_profile(model, effort, executable, purpose="run"):
    if purpose not in {"run","readiness"}:raise CallerRoutingRejected("unsupported owner profile purpose")
    inventory=INVENTORY if purpose=="run" else READINESS_INVENTORY
    if (model, effort) != ('gpt-6-luna', 'xhigh'):
        raise CallerRoutingRejected('session private host permits only the exact ordinary S05 pair')
    parent = inventory.parent.lstat()
    if parent.st_uid != os.getuid() or stat.S_IMODE(parent.st_mode) != 0o700:
        raise CallerRoutingRejected('private host control parent must be owner-only')
    inventory_sha = _stable(inventory)
    raw = inventory.read_bytes()
    if len(raw) > 16384 or hashlib.sha256(raw).hexdigest() != inventory_sha:
        raise CallerRoutingRejected('private host inventory unstable or oversized')
    data = _decode(raw)
    fields = {'schema','host_class','executable','executable_sha256','build_manifest','build_manifest_sha256','argv','configured_model','configured_effort','root_reviewed_startup_no_tools','private_runtime_home','readonly_auth_source_home','readonly_auth_source_backend','readonly_auth_keyring_backend_kind','environment'}
    if not isinstance(data,dict) or set(data) != fields or data['schema'] != SCHEMA or data['host_class'] != 'OWNED_PRIVATE_APPSERVER':
        raise CallerRoutingRejected('unsupported private host inventory')
    if data['environment']!=PRIVATE_STACK_ENV or type(data['environment']) is not dict:
        raise CallerRoutingRejected('private environment must contain only exact bounded RUST_MIN_STACK')
    if data['root_reviewed_startup_no_tools'] is not True or (data['configured_model'],data['configured_effort']) != (model,effort):
        raise CallerRoutingRejected('private host startup/model inventory not accepted')
    if executable != data['executable'] or _stable(Path(executable),executable=True) != data['executable_sha256']:
        raise CallerRoutingRejected('private host executable inventory differs')
    if _stable(Path(data['build_manifest'])) != data['build_manifest_sha256']:
        raise CallerRoutingRejected('private host build manifest differs')
    argv=data['argv']
    if (not isinstance(argv,list) or not argv or argv[0] != executable
            or any(not isinstance(x,str) or '\x00' in x for x in argv)):
        raise CallerRoutingRejected('private host exact argv inventory invalid')
    home=Path(data['private_runtime_home']);_path(home)
    node=home.lstat()
    if not home.is_absolute() or not stat.S_ISDIR(node.st_mode) or node.st_uid!=os.getuid() or stat.S_IMODE(node.st_mode)!=0o700:
        raise CallerRoutingRejected('private runtime home must be existing owner-only directory')
    auth=Path(data['readonly_auth_source_home']);_path(auth)
    if not auth.is_absolute() or not auth.is_dir() or data['readonly_auth_source_backend'] not in {'file','keyring'} or data['readonly_auth_keyring_backend_kind'] not in {'direct','secrets'}:
        raise CallerRoutingRejected('private readonly auth source inventory differs')
    expected=[executable,'--s05-owned-session','--private-runtime-home',str(home),'--readonly-auth-source-home',str(auth),'--readonly-auth-source-backend',data['readonly_auth_source_backend'],'--readonly-auth-keyring-backend-kind',data['readonly_auth_keyring_backend_kind']]
    if argv!=expected:
        raise CallerRoutingRejected('private exact owned argv differs from supported profile')
    # The reviewed inventory fixes startup arguments; the observer independently
    # verifies configured model/effort/approval/sandbox/cwd and resolved surface.
    return PrivateOwnedLaunchProfile(tuple(argv),hashlib.sha256(_json({'argv':argv,'environment':data['environment']}).encode()).hexdigest(),0,0,tuple(data['environment'].items())),data['executable_sha256']


def private_profile(model, effort, executable, *, purpose="run"):
    try:
        return _private_profile(model, effort, executable,purpose)
    except CallerRoutingRejected:
        raise
    except Exception:
        raise CallerRoutingRejected("private host owner inventory unavailable or invalid") from None

def private_runtime_home(profile):
    # Profile is freshly validated by owner inventory, never caller argv.
    return profile.argv[profile.argv.index('--private-runtime-home')+1]
