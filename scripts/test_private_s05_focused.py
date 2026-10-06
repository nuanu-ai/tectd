"""Explicit bounded suite; excludes unrelated historical one-off dependencies."""
import unittest
MODULES=['scripts.test_private_s05_readiness','scripts.test_private_s05_acceptance','scripts.test_private_s05_adapter','scripts.test_private_s05_evidence','scripts.test_private_s05_seal','scripts.test_private_owned_appserver_profile','scripts.test_authenticated_caller_source','scripts.test_caller_host_routing','scripts.test_codex_app_server_observer','scripts.test_codex_app_server_profile','scripts.test_codex_app_server_rpc']
EXCLUDED={'test_one_off_exact_configuration_gates_and_durable_replay','test_one_off_failed_model_or_surface_gate_never_starts_thread'}
def flatten(suite):
    for test in suite:
        if isinstance(test,unittest.TestSuite):yield from flatten(test)
        else:yield test
def load_tests(loader,tests,pattern):
    return unittest.TestSuite(t for t in flatten(loader.loadTestsFromNames(MODULES)) if t._testMethodName not in EXCLUDED)
if __name__=='__main__':unittest.main()
