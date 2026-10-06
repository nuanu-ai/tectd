import hashlib,json,tempfile,unittest,os,sys
from pathlib import Path
from unittest.mock import patch
from scripts import private_owned_appserver_profile as private
from scripts.caller_host_routing import CallerRoutingRejected

class PrivateInventoryTests(unittest.TestCase):
    def setUp(self):
        self.temp=tempfile.TemporaryDirectory(dir="/private/tmp");self.root=Path(self.temp.name);self.root.chmod(0o700)
        self.binary=self.root/'host';self.binary.write_bytes(b'nonexecuted-test-fixture');self.binary.chmod(0o500)
        self.build=self.root/'build.json';self.build.write_text('{}');self.build.chmod(0o400)
        self.inventory=self.root/'private-host.json'
        self.data={'schema':private.SCHEMA,'host_class':'OWNED_PRIVATE_APPSERVER','executable':str(self.binary),'executable_sha256':hashlib.sha256(self.binary.read_bytes()).hexdigest(),'build_manifest':str(self.build),'build_manifest_sha256':hashlib.sha256(self.build.read_bytes()).hexdigest(),'argv':[str(self.binary),'--test-fixture-never-executed'],'configured_model':'gpt-6-luna','configured_effort':'xhigh','root_reviewed_startup_no_tools':True}
        self.data['environment']={'RUST_MIN_STACK':'8388608'}
        self.data.update(private_runtime_home=str(self.root),readonly_auth_source_home=str(self.root),readonly_auth_source_backend='file',readonly_auth_keyring_backend_kind='direct')
        self.data['argv']=[str(self.binary),'--s05-owned-session','--private-runtime-home',str(self.root),'--readonly-auth-source-home',str(self.root),'--readonly-auth-source-backend','file','--readonly-auth-keyring-backend-kind','direct']
        self.write();self.patch=patch.object(private,'INVENTORY',self.inventory);self.patch.start()
    def tearDown(self):self.patch.stop();self.temp.cleanup()
    def write(self):
        if self.inventory.exists():self.inventory.chmod(0o600)
        self.inventory.write_text(json.dumps(self.data));self.inventory.chmod(0o400)
    def run_profile(self):return private.private_profile('gpt-6-luna','xhigh',str(self.binary))
    def test_exact_frozen_inventory_passes_without_process(self):
        profile,sha=self.run_profile();self.assertEqual(profile.argv[0],str(self.binary));self.assertEqual(sha,self.data['executable_sha256'])
    def test_missing_nonexact_or_extra_private_environment_denied(self):
        for environment in (None,{}, {'RUST_MIN_STACK':'16777216'}, {'RUST_MIN_STACK':'8388608','OPENAI_API_KEY':'forbidden'}):
            self.data['environment']=environment;self.write()
            with self.assertRaises(CallerRoutingRejected):self.run_profile()
        del self.data['environment'];self.write()
        with self.assertRaises(CallerRoutingRejected):self.run_profile()
    def test_real_subprocess_receives_only_pinned_stack_value_and_failure_captured(self):
        from scripts.codex_app_server_rpc import OwnedAppServerRpc
        from scripts.private_s05_readiness import execute_probe
        profile,_=self.run_profile();environment=private.private_environment(profile);before=dict(os.environ)
        program='import json,os,sys; r=json.loads(sys.stdin.readline()); print(json.dumps({"id":r["id"],"result":{"value":os.environ["RUST_MIN_STACK"]}}),flush=True); sys.stdin.read()'
        with OwnedAppServerRpc([sys.executable,'-c',program],str(self.root),private_stack_environment=environment) as rpc:
            response=rpc.initialize({'name':'stack-only-fixture','version':'1'},timeout=5)
            self.assertEqual(response,{'value':'8388608'});self.assertEqual(rpc.seal_notifications(timeout=5),[])
        failure='import os,sys; sys.stderr.write(os.environ["RUST_MIN_STACK"]); sys.stderr.flush(); sys.exit(23)'
        result,error=execute_probe([sys.executable,'-c',failure],str(self.root),private_stack_environment=environment)
        self.assertIsNotNone(error);self.assertEqual(result['child_diagnostics']['child_exit_code'],23);self.assertEqual(result['child_diagnostics']['stderr_redacted'],'8388608');self.assertEqual(os.environ,before)
        for bad in ({},{'RUST_MIN_STACK':'4194304'},{'RUST_MIN_STACK':'8388608','CODEX_HOME':'forbidden'}):
            with self.assertRaises(ValueError):OwnedAppServerRpc([sys.executable,'-c',program],str(self.root),private_stack_environment=bad)
        with self.assertRaises(CallerRoutingRejected):private.private_environment(None)
    def test_wrong_pair_denied(self):
        with self.assertRaises(CallerRoutingRejected):private.private_profile('gpt-6.1-sol','medium',str(self.binary))
    def test_writable_inventory_denied(self):
        self.inventory.chmod(0o600)
        with self.assertRaises(CallerRoutingRejected):self.run_profile()
    def test_unreviewed_startup_denied(self):
        self.data['root_reviewed_startup_no_tools']=False;self.write()
        with self.assertRaises(CallerRoutingRejected):self.run_profile()
    def test_changed_binary_denied(self):
        self.binary.chmod(0o700);self.binary.write_bytes(b'changed');self.binary.chmod(0o500)
        with self.assertRaises(CallerRoutingRejected):self.run_profile()
    def test_changed_build_manifest_denied(self):
        self.build.chmod(0o600);self.build.write_text('{"changed":true}');self.build.chmod(0o400)
        with self.assertRaises(CallerRoutingRejected):self.run_profile()
    def test_caller_path_cannot_select_host(self):
        with self.assertRaises(CallerRoutingRejected):private.private_profile('gpt-6-luna','xhigh','/bin/sh')
    def test_fresh_fixed_slots_are_distinct_and_never_select_old_inventory(self):
        from scripts.private_s05_readiness import READINESS_LEDGER
        # The fixed ordinary inventory cannot fall back to a valid old slot.
        fresh=self.root/'private-host-ordinary-03.json'
        old_bytes=self.inventory.read_bytes()
        with patch.object(private,'INVENTORY',fresh):
            with self.assertRaises(CallerRoutingRejected):self.run_profile()
        self.assertEqual(self.inventory.read_bytes(),old_bytes)
        self.assertFalse(fresh.exists())
        self.assertEqual(private.READINESS_INVENTORY.name,'private-host-readiness-07.json')
        self.assertEqual(READINESS_LEDGER.name,'readiness-ledger-07')
if __name__=='__main__':unittest.main()
