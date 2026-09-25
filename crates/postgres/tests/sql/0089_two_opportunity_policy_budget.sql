-- Run only in a disposable PG18 database inside BEGIN/ROLLBACK after 0089.
-- Replica mode seeds synthetic parent rows; all admission assertions run with
-- origin triggers enabled. No provider call or durable row is made.
SET LOCAL tect.tenant_id = '11111111-1111-1111-1111-111111111111';
SET LOCAL session_replication_role = replica;
INSERT INTO advisory_budget_policies
 (tenant_id,workspace_id,id,version,digest,effective_from_unix_ms,effective_until_unix_ms,
  provider_calls,input_tokens,output_tokens,request_utf8_bytes,elapsed_monotonic_ms,
  retry_dispatches,approved_by,approval_signature)
VALUES
 ('11111111-1111-1111-1111-111111111111','22222222-2222-2222-2222-222222222222',
  '44444444-4444-4444-4444-444444444444',1,repeat('a',64),0,9223372036854775807,
  1,100,100,100,100,1,'33333333-3333-3333-3333-333333333333',repeat('a',128)),
 ('11111111-1111-1111-1111-111111111111','22222222-2222-2222-2222-222222222222',
  '55555555-5555-5555-5555-555555555555',2,repeat('b',64),0,9223372036854775807,
  2,100,100,100,100,1,'33333333-3333-3333-3333-333333333333',repeat('b',128)),
 ('11111111-1111-1111-1111-111111111111','22222222-2222-2222-2222-222222222222',
  '66666666-6666-6666-6666-666666666666',3,repeat('e',64),0,9223372036854775807,
  1,100,100,100,100,1,'33333333-3333-3333-3333-333333333333',repeat('e',128));
INSERT INTO advisory_dispatch
 (id,tenant_id,workspace_id,opportunity_id,attempt_number,provider,model,
  configuration_snapshot,configuration_digest,material_digest,payload_digest,
  request_payload,state,send_certainty,retry_basis)
SELECT ('00000000-0000-0000-0000-' || lpad(n::text,12,'0'))::uuid,
       '11111111-1111-1111-1111-111111111111'::uuid,
       '22222222-2222-2222-2222-222222222222'::uuid,
       ('00000000-0000-0000-0001-' || lpad(n::text,12,'0'))::uuid,
       1,'test','test','{}'::jsonb,repeat('c',64),repeat('d',64),
       encode(sha256(convert_to('x','UTF8')),'hex'),convert_to('x','UTF8'),
       'authorized','not_sent','initial'
FROM generate_series(1,5) AS n;
SET LOCAL session_replication_role = origin;

-- First policy: one consumed call on opportunity 1; opportunity 2 must fail.
INSERT INTO advisory_budget_reservations
 (tenant_id,workspace_id,opportunity_id,dispatch_id,policy_id,policy_version,policy_digest,
  policy_effective_from_unix_ms,policy_effective_until_unix_ms,request_sha256,
  request_utf8_bytes,reserved_retry_dispatches,remaining_elapsed_ms,
  reserved_input_tokens,reserved_output_tokens)
VALUES
 ('11111111-1111-1111-1111-111111111111','22222222-2222-2222-2222-222222222222',
  '00000000-0000-0000-0001-000000000001','00000000-0000-0000-0000-000000000001',
  '44444444-4444-4444-4444-444444444444',1,repeat('a',64),0,9223372036854775807,
  encode(sha256(convert_to('x','UTF8')),'hex'),1,0,100,100,100);
SET LOCAL session_replication_role = replica;
INSERT INTO advisory_budget_consumptions
 (tenant_id,workspace_id,opportunity_id,dispatch_id,policy_id,policy_version,policy_digest,
  request_sha256,request_utf8_bytes,send_certainty,outcome,input_tokens,output_tokens,
  input_tokens_known,output_tokens_known,monotonic_elapsed_ms,elapsed_known,
  calls,retry_dispatches,unknown_usage,exhausted_after_response,sealed_at)
VALUES
 ('11111111-1111-1111-1111-111111111111','22222222-2222-2222-2222-222222222222',
  '00000000-0000-0000-0001-000000000001','00000000-0000-0000-0000-000000000001',
  '44444444-4444-4444-4444-444444444444',1,repeat('a',64),
  encode(sha256(convert_to('x','UTF8')),'hex'),1,'sent_unknown','provider_failure',
  1,1,true,true,1,true,1,0,false,false,clock_timestamp());
SET LOCAL session_replication_role = origin;
DO $test$
BEGIN
    BEGIN
        INSERT INTO advisory_budget_reservations
         (tenant_id,workspace_id,opportunity_id,dispatch_id,policy_id,policy_version,policy_digest,
          policy_effective_from_unix_ms,policy_effective_until_unix_ms,request_sha256,
          request_utf8_bytes,reserved_retry_dispatches,remaining_elapsed_ms,
          reserved_input_tokens,reserved_output_tokens)
        VALUES
         ('11111111-1111-1111-1111-111111111111','22222222-2222-2222-2222-222222222222',
          '00000000-0000-0000-0001-000000000002','00000000-0000-0000-0000-000000000002',
          '44444444-4444-4444-4444-444444444444',1,repeat('a',64),0,9223372036854775807,
          encode(sha256(convert_to('x','UTF8')),'hex'),1,0,99,99,99);
        RAISE EXCEPTION 'second opportunity improperly passed one-call policy';
    EXCEPTION WHEN check_violation THEN
        IF SQLERRM <> 'global budget exhausted before dispatch' THEN RAISE; END IF;
    END;
END $test$;

-- A consumed Anti-Bloat attempt spends the same policy as a later shared
-- advisory dispatch on a different opportunity.
SET LOCAL session_replication_role = replica;
INSERT INTO scope_anti_bloat_budget_reservations
 (tenant_id,workspace_id,opportunity_id,review_id,policy_id,policy_version,policy_digest,
  request_sha256,request_utf8_bytes,reserved_input_tokens,reserved_output_tokens,reserved_elapsed_ms)
VALUES
 ('11111111-1111-1111-1111-111111111111','22222222-2222-2222-2222-222222222222',
  '00000000-0000-0000-0002-000000000001','77777777-7777-7777-7777-777777777777',
  '66666666-6666-6666-6666-666666666666',3,repeat('e',64),
  encode(sha256(convert_to('x','UTF8')),'hex'),1,100,100,100);
INSERT INTO scope_anti_bloat_budget_consumptions
 (tenant_id,workspace_id,review_id,policy_id,policy_version,policy_digest,
  request_sha256,input_tokens,output_tokens,elapsed_monotonic_ms,
  unknown_usage,exhausted_after_response,transport_failed)
VALUES
 ('11111111-1111-1111-1111-111111111111','22222222-2222-2222-2222-222222222222',
  '77777777-7777-7777-7777-777777777777',
  '66666666-6666-6666-6666-666666666666',3,repeat('e',64),
  encode(sha256(convert_to('x','UTF8')),'hex'),1,1,1,false,false,false);
SET LOCAL session_replication_role = origin;
DO $test$
BEGIN
    BEGIN
        INSERT INTO advisory_budget_reservations
         (tenant_id,workspace_id,opportunity_id,dispatch_id,policy_id,policy_version,policy_digest,
          policy_effective_from_unix_ms,policy_effective_until_unix_ms,request_sha256,
          request_utf8_bytes,reserved_retry_dispatches,remaining_elapsed_ms,
          reserved_input_tokens,reserved_output_tokens)
        VALUES
         ('11111111-1111-1111-1111-111111111111','22222222-2222-2222-2222-222222222222',
          '00000000-0000-0000-0001-000000000005','00000000-0000-0000-0000-000000000005',
          '66666666-6666-6666-6666-666666666666',3,repeat('e',64),0,9223372036854775807,
          encode(sha256(convert_to('x','UTF8')),'hex'),1,0,99,99,99);
        RAISE EXCEPTION 'shared dispatch ignored Anti-Bloat call';
    EXCEPTION WHEN check_violation THEN
        IF SQLERRM <> 'global budget exhausted before dispatch' THEN RAISE; END IF;
    END;
END $test$;

-- Separate two-call policy: opportunities 3 and 4 are admitted in sequence.
INSERT INTO advisory_budget_reservations
 (tenant_id,workspace_id,opportunity_id,dispatch_id,policy_id,policy_version,policy_digest,
  policy_effective_from_unix_ms,policy_effective_until_unix_ms,request_sha256,
  request_utf8_bytes,reserved_retry_dispatches,remaining_elapsed_ms,
  reserved_input_tokens,reserved_output_tokens)
VALUES
 ('11111111-1111-1111-1111-111111111111','22222222-2222-2222-2222-222222222222',
  '00000000-0000-0000-0001-000000000003','00000000-0000-0000-0000-000000000003',
  '55555555-5555-5555-5555-555555555555',2,repeat('b',64),0,9223372036854775807,
  encode(sha256(convert_to('x','UTF8')),'hex'),1,0,100,100,100);
SET LOCAL session_replication_role = replica;
INSERT INTO advisory_budget_consumptions
 (tenant_id,workspace_id,opportunity_id,dispatch_id,policy_id,policy_version,policy_digest,
  request_sha256,request_utf8_bytes,send_certainty,outcome,input_tokens,output_tokens,
  input_tokens_known,output_tokens_known,monotonic_elapsed_ms,elapsed_known,
  calls,retry_dispatches,unknown_usage,exhausted_after_response,sealed_at)
VALUES
 ('11111111-1111-1111-1111-111111111111','22222222-2222-2222-2222-222222222222',
  '00000000-0000-0000-0001-000000000003','00000000-0000-0000-0000-000000000003',
  '55555555-5555-5555-5555-555555555555',2,repeat('b',64),
  encode(sha256(convert_to('x','UTF8')),'hex'),1,'sent_unknown','provider_failure',
  1,1,true,true,1,true,1,0,false,false,clock_timestamp());
SET LOCAL session_replication_role = origin;
INSERT INTO advisory_budget_reservations
 (tenant_id,workspace_id,opportunity_id,dispatch_id,policy_id,policy_version,policy_digest,
  policy_effective_from_unix_ms,policy_effective_until_unix_ms,request_sha256,
  request_utf8_bytes,reserved_retry_dispatches,remaining_elapsed_ms,
  reserved_input_tokens,reserved_output_tokens)
VALUES
 ('11111111-1111-1111-1111-111111111111','22222222-2222-2222-2222-222222222222',
  '00000000-0000-0000-0001-000000000004','00000000-0000-0000-0000-000000000004',
  '55555555-5555-5555-5555-555555555555',2,repeat('b',64),0,9223372036854775807,
  encode(sha256(convert_to('x','UTF8')),'hex'),1,0,99,99,99);
DO $test$
BEGIN
    IF (SELECT count(*) FROM advisory_budget_reservations WHERE policy_id='55555555-5555-5555-5555-555555555555')<>2 THEN
        RAISE EXCEPTION 'two-call policy did not admit two opportunities';
    END IF;
END $test$;
