-- Run inside BEGIN/ROLLBACK on a disposable PG18 database as its owner.
-- Set test_runtime_role with psql -v to the role passed to admin::migrate.
SET LOCAL tect.tenant_id = '11111111-1111-1111-1111-111111111111';
ALTER TABLE advisory_budget_policies DISABLE TRIGGER advisory_budget_policy_guard_trigger;
SET LOCAL session_replication_role = replica;
INSERT INTO advisory_budget_policies
 (tenant_id,workspace_id,id,version,digest,effective_from_unix_ms,effective_until_unix_ms,
  provider_calls,input_tokens,output_tokens,request_utf8_bytes,elapsed_monotonic_ms,
  retry_dispatches,approved_by,approval_signature)
VALUES
 ('11111111-1111-1111-1111-111111111111','22222222-2222-2222-2222-222222222222',
  '44444444-4444-4444-4444-444444444444',1,repeat('a',64),0,9223372036854775807,
  1,100,100,100,100,1,'33333333-3333-3333-3333-333333333333',repeat('a',128));
SET LOCAL session_replication_role = origin;
ALTER TABLE advisory_budget_policies ENABLE ALWAYS TRIGGER advisory_budget_policy_guard_trigger;
GRANT UPDATE(id) ON TABLE advisory_budget_policies TO :"test_runtime_role";
SET LOCAL ROLE :"test_runtime_role";
DO $test$
BEGIN
    PERFORM id FROM advisory_budget_policies
      WHERE (tenant_id,workspace_id,id)=
      ('11111111-1111-1111-1111-111111111111',
       '22222222-2222-2222-2222-222222222222',
       '44444444-4444-4444-4444-444444444444') FOR UPDATE;
    IF NOT FOUND THEN RAISE EXCEPTION 'runtime policy lock did not find row'; END IF;
    BEGIN
        UPDATE advisory_budget_policies SET id=id
          WHERE id='44444444-4444-4444-4444-444444444444';
        RAISE EXCEPTION 'runtime changed immutable policy';
    EXCEPTION WHEN check_violation THEN
        IF SQLERRM <> 'advisory budget policy is immutable' THEN RAISE; END IF;
    END;
END $test$;
