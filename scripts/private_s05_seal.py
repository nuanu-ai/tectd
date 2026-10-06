"""Private one-use launch/turn fences and exact independent saved-history sealing.

Reservations record intent, never successful sends. A failed/uncertain reserved
invocation is consumed. This module cannot authenticate JSON or start a host.
"""
from __future__ import annotations
import hashlib
import os
from pathlib import Path
from scripts.bounded_caller_route_launcher import _private_ledger
from scripts.codex_app_server_observer import _Ledger, _sha
from scripts.private_s05_evidence import PrivateS05EvidenceRejected

class OneFreshPrivateTurn:
    def __init__(self):
        self._start_claimed=False
        self._thread_id=None
        self._turn_claimed=False
        self._turn_id=None
    def claim_start(self):
        if self._start_claimed:
            raise PrivateS05EvidenceRejected('private thread start already claimed; no second start or retry')
        self._start_claimed=True
    def record_thread(self,thread_id):
        if not self._start_claimed or self._thread_id is not None or not isinstance(thread_id,str) or not thread_id.strip():
            raise PrivateS05EvidenceRejected('exactly one minted private thread required')
        self._thread_id=thread_id
    def require_thread(self,thread_id):
        if self._thread_id is None or thread_id != self._thread_id:
            raise PrivateS05EvidenceRejected('foreign private thread denied')
    def claim_turn(self,thread_id):
        self.require_thread(thread_id)
        if self._turn_claimed:
            raise PrivateS05EvidenceRejected('private turn send already claimed; no retry')
        self._turn_claimed=True
    def record_turn(self,thread_id,turn_id):
        self.require_thread(thread_id)
        if not self._turn_claimed or self._turn_id is not None or not isinstance(turn_id,str) or not turn_id.strip():
            raise PrivateS05EvidenceRejected('exactly one accepted private turn required')
        self._turn_id=turn_id
    def require_read(self,thread_id,turn_id):
        self.require_thread(thread_id)
        if self._turn_id is None or turn_id != self._turn_id:
            raise PrivateS05EvidenceRejected('foreign private turn read denied')

class PrivateSealLedger:
    def __init__(self,directory:Path):
        self.directory=directory
        self._identity=_private_ledger(directory)
        self._key=None
        self._intent_digest=None
    def _ledger(self):
        if _private_ledger(self.directory)!=self._identity:
            raise PrivateS05EvidenceRejected('private invocation ledger changed identity')
        return _Ledger(self.directory)
    def reserve(self,*,invocation_key,intent_digest,prompt_sha256,source_binding_sha256,profile_sha256,intent=None):
        if self._key is not None:
            raise PrivateS05EvidenceRejected('private observer invocation already reserved')
        if not isinstance(invocation_key,str) or not invocation_key.strip():
            raise PrivateS05EvidenceRejected('bounded private invocation key required')
        for digest in (intent_digest,prompt_sha256,source_binding_sha256,profile_sha256):
            if not isinstance(digest,str) or len(digest)!=64 or any(c not in '0123456789abcdef' for c in digest):
                raise PrivateS05EvidenceRejected('exact private invocation digests required')
        key=_sha(invocation_key);ledger=self._ledger()
        try:
            ledger.write(key+'.00-reserved.json',{'stage':'private_s05_reserved_intent_not_send','invocation_key':invocation_key,'intent_digest':intent_digest,'prompt_sha256':prompt_sha256,'source_binding_sha256':source_binding_sha256,'profile_sha256':profile_sha256,'intent':intent})
        except FileExistsError:
            raise PrivateS05EvidenceRejected('private invocation key consumed; no new start or retry') from None
        finally:ledger.close()
        self._key=key;self._intent_digest=intent_digest
    def record(self,stage,value):
        if self._key is None or stage not in {"01-send-claimed","03-result","04-receipt"}:
            raise PrivateS05EvidenceRejected("invalid reserved private ledger stage")
        ledger=self._ledger()
        try:ledger.write(self._key+"."+stage+".json",value)
        finally:ledger.close()
    def seal_rollout(self,raw:bytes,*,expected_sha256):
        return self._seal_bytes(raw,expected_sha256=expected_sha256,name_suffix=".02-rollout.jsonl",limit=16*1024*1024)
    def seal_history(self,raw:bytes,*,expected_sha256):
        return self._seal_bytes(raw,expected_sha256=expected_sha256,name_suffix=".02-history.json",limit=262144)
    def _seal_bytes(self,raw,*,expected_sha256,name_suffix,limit):
        if self._key is None or not isinstance(raw,bytes) or len(raw)>limit or hashlib.sha256(raw).hexdigest()!=expected_sha256:
            raise PrivateS05EvidenceRejected('exact bounded independently checked private history required')
        ledger=self._ledger();name=self._key+name_suffix
        try:
            fd=os.open(name,os.O_WRONLY|os.O_CREAT|os.O_EXCL|os.O_NOFOLLOW,0o600,dir_fd=ledger.fd)
            with os.fdopen(fd,'wb') as stream:
                stream.write(raw);stream.flush();os.fsync(stream.fileno())
            os.fsync(ledger.fd)
            fd=os.open(name,os.O_RDONLY|os.O_NOFOLLOW,dir_fd=ledger.fd)
            with os.fdopen(fd,'rb') as stream:read=stream.read(limit+1)
            if read!=raw or hashlib.sha256(read).hexdigest()!=expected_sha256:
                raise PrivateS05EvidenceRejected('private saved-history independent readback differs')
            return name
        except FileExistsError:
            raise PrivateS05EvidenceRejected('private history seal already exists; no rewrite') from None
        finally:ledger.close()
