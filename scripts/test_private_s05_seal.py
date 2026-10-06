import hashlib,tempfile,unittest
from pathlib import Path
from scripts.private_s05_evidence import PrivateS05EvidenceRejected
from scripts.private_s05_seal import OneFreshPrivateTurn,PrivateSealLedger

class OneFreshTurnTests(unittest.TestCase):
    def test_minted_thread_and_turn_only(self):
        guard=OneFreshPrivateTurn();guard.claim_start();guard.record_thread('minted');guard.claim_turn('minted');guard.record_turn('minted','accepted');guard.require_read('minted','accepted')
        for ids in [('foreign','accepted'),('minted','foreign')]:
            with self.assertRaises(PrivateS05EvidenceRejected):guard.require_read(*ids)
    def test_second_start_and_retry_denied_before_any_send(self):
        guard=OneFreshPrivateTurn();guard.claim_start()
        with self.assertRaises(PrivateS05EvidenceRejected):guard.claim_start()
        guard.record_thread('minted');guard.claim_turn('minted')
        with self.assertRaises(PrivateS05EvidenceRejected):guard.claim_turn('minted')
    def test_foreign_thread_cannot_claim_turn(self):
        guard=OneFreshPrivateTurn();guard.claim_start();guard.record_thread('minted')
        with self.assertRaises(PrivateS05EvidenceRejected):guard.claim_turn('foreign')

class LedgerSealTests(unittest.TestCase):
    def setUp(self):self.temp=tempfile.TemporaryDirectory(dir='/private/tmp');self.root=Path(self.temp.name);self.root.chmod(0o700)
    def tearDown(self):self.temp.cleanup()
    def reserve(self,ledger):ledger.reserve(invocation_key='fresh-private-key',intent_digest='a'*64,prompt_sha256='b'*64,source_binding_sha256='c'*64,profile_sha256='d'*64)
    def test_second_observer_cannot_reuse_durable_invocation(self):
        first=PrivateSealLedger(self.root);self.reserve(first)
        with self.assertRaises(PrivateS05EvidenceRejected):self.reserve(PrivateSealLedger(self.root))
        self.assertEqual(len(list(self.root.iterdir())),1)
    def test_exact_history_saved_and_independently_read_without_rewrite(self):
        ledger=PrivateSealLedger(self.root);self.reserve(ledger);raw=b'{"actual_saved_history":true}'
        digest=hashlib.sha256(raw).hexdigest();name=ledger.seal_history(raw,expected_sha256=digest);saved=self.root/name;before=saved.stat();self.assertEqual(saved.read_bytes(),raw)
        with self.assertRaises(PrivateS05EvidenceRejected):ledger.seal_history(raw,expected_sha256=digest)
        after=saved.stat();self.assertEqual((before.st_ino,before.st_mtime_ns,before.st_size),(after.st_ino,after.st_mtime_ns,after.st_size))
    def test_unchecked_history_and_unreserved_seal_denied(self):
        ledger=PrivateSealLedger(self.root)
        with self.assertRaises(PrivateS05EvidenceRejected):ledger.seal_history(b'{}',expected_sha256=hashlib.sha256(b'{}').hexdigest())
        self.reserve(ledger)
        with self.assertRaises(PrivateS05EvidenceRejected):ledger.seal_history(b'{}',expected_sha256='f'*64)
if __name__=='__main__':unittest.main()
