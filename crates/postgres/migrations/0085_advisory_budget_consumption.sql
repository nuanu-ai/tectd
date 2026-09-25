-- Per-network-attempt consumption is append-only and follows the raw seal.
ALTER TABLE advisory_budget_reservations
    ADD COLUMN reserved_input_tokens bigint,
    ADD COLUMN reserved_output_tokens bigint;
ALTER TABLE advisory_budget_reservations
    DISABLE TRIGGER advisory_budget_reservation_guard_trigger;
UPDATE advisory_budget_reservations r SET
    reserved_input_tokens=p.input_tokens,
    reserved_output_tokens=p.output_tokens
FROM advisory_budget_policies p WHERE
    (r.tenant_id,r.workspace_id,r.policy_id)=(p.tenant_id,p.workspace_id,p.id);
ALTER TABLE advisory_budget_reservations
    ENABLE TRIGGER advisory_budget_reservation_guard_trigger;
ALTER TABLE advisory_budget_reservations
    ALTER COLUMN reserved_input_tokens SET NOT NULL,
    ALTER COLUMN reserved_output_tokens SET NOT NULL,
    ADD CONSTRAINT advisory_budget_reserved_input_positive CHECK (reserved_input_tokens > 0),
    ADD CONSTRAINT advisory_budget_reserved_output_positive CHECK (reserved_output_tokens > 0);

CREATE TABLE advisory_budget_consumptions (
    tenant_id uuid NOT NULL,
    workspace_id uuid NOT NULL,
    opportunity_id uuid NOT NULL,
    dispatch_id uuid NOT NULL,
    policy_id uuid NOT NULL,
    policy_version bigint NOT NULL,
    policy_digest text NOT NULL,
    request_sha256 text NOT NULL,
    request_utf8_bytes bigint NOT NULL CHECK (request_utf8_bytes > 0),
    response_sha256 text CHECK (response_sha256 ~ '^[0-9a-f]{64}$'),
    raw_response_ref text,
    send_certainty text NOT NULL CHECK (send_certainty IN ('sent','sent_unknown','not_sent')),
    outcome text NOT NULL CHECK (outcome IN ('provider_response','provider_failure')),
    input_tokens bigint CHECK (input_tokens >= 0),
    output_tokens bigint CHECK (output_tokens >= 0),
    input_tokens_known boolean NOT NULL,
    output_tokens_known boolean NOT NULL,
    monotonic_elapsed_ms bigint CHECK (monotonic_elapsed_ms >= 0),
    elapsed_known boolean NOT NULL,
    calls bigint NOT NULL CHECK (calls = 1),
    retry_dispatches bigint NOT NULL CHECK (retry_dispatches IN (0,1)),
    unknown_usage boolean NOT NULL,
    exhausted_after_response boolean NOT NULL,
    sealed_at timestamptz NOT NULL,
    recorded_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
    PRIMARY KEY (tenant_id,workspace_id,dispatch_id),
    FOREIGN KEY (tenant_id,workspace_id,dispatch_id)
        REFERENCES advisory_budget_reservations (tenant_id,workspace_id,dispatch_id),
    FOREIGN KEY (tenant_id,workspace_id,opportunity_id,dispatch_id)
        REFERENCES advisory_dispatch (tenant_id,workspace_id,opportunity_id,id),
    CHECK (input_tokens_known = (input_tokens IS NOT NULL)),
    CHECK (output_tokens_known = (output_tokens IS NOT NULL)),
    CHECK (elapsed_known = (monotonic_elapsed_ms IS NOT NULL)),
    CHECK (unknown_usage = (NOT input_tokens_known OR NOT output_tokens_known OR NOT elapsed_known))
);
CREATE INDEX advisory_budget_consumptions_opportunity_idx
    ON advisory_budget_consumptions (tenant_id,workspace_id,opportunity_id);

CREATE FUNCTION advisory_budget_consumption_guard() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $guard$
DECLARE dispatch public.advisory_dispatch%ROWTYPE;
DECLARE reservation public.advisory_budget_reservations%ROWTYPE;
BEGIN
    IF TG_OP <> 'INSERT' THEN
        RAISE EXCEPTION 'budget consumption is immutable' USING ERRCODE='23514';
    END IF;
    IF NEW.tenant_id IS DISTINCT FROM
       NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid THEN
        RAISE EXCEPTION 'budget consumption tenant mismatch' USING ERRCODE='42501';
    END IF;
    SELECT * INTO dispatch FROM public.advisory_dispatch WHERE
      (tenant_id,workspace_id,opportunity_id,id)=
      (NEW.tenant_id,NEW.workspace_id,NEW.opportunity_id,NEW.dispatch_id);
    SELECT * INTO reservation FROM public.advisory_budget_reservations WHERE
      (tenant_id,workspace_id,dispatch_id)=
      (NEW.tenant_id,NEW.workspace_id,NEW.dispatch_id);
    IF dispatch.id IS NULL OR reservation.dispatch_id IS NULL
       OR dispatch.state <> 'sealed' OR dispatch.sealed_at IS NULL
       OR NEW.policy_id <> reservation.policy_id
       OR NEW.policy_version <> reservation.policy_version
       OR NEW.policy_digest <> reservation.policy_digest
       OR NEW.request_sha256 <> reservation.request_sha256
       OR NEW.request_utf8_bytes <> reservation.request_utf8_bytes
       OR (NEW.input_tokens IS NOT NULL AND NEW.input_tokens > reservation.reserved_input_tokens
           AND NOT NEW.exhausted_after_response)
       OR (NEW.output_tokens IS NOT NULL AND NEW.output_tokens > reservation.reserved_output_tokens
           AND NOT NEW.exhausted_after_response)
       OR NEW.calls <> reservation.reserved_calls
       OR NEW.retry_dispatches <> reservation.reserved_retry_dispatches
       OR NEW.send_certainty <> dispatch.send_certainty
       OR NEW.outcome <> dispatch.outcome
       OR NEW.response_sha256 IS DISTINCT FROM
          CASE WHEN dispatch.response_payload IS NULL THEN NULL ELSE
            pg_catalog.encode(pg_catalog.sha256(dispatch.response_payload),'hex') END
       OR NEW.raw_response_ref IS DISTINCT FROM dispatch.raw_response_ref
       OR NEW.input_tokens IS DISTINCT FROM dispatch.input_tokens
       OR NEW.output_tokens IS DISTINCT FROM dispatch.output_tokens
       OR NEW.monotonic_elapsed_ms IS DISTINCT FROM dispatch.latency_ms
       OR NEW.sealed_at IS DISTINCT FROM dispatch.sealed_at THEN
        RAISE EXCEPTION 'budget consumption seal mismatch' USING ERRCODE='23514';
    END IF;
    RETURN NEW;
END $guard$;
CREATE TRIGGER advisory_budget_consumption_guard_trigger
    BEFORE INSERT OR UPDATE OR DELETE ON advisory_budget_consumptions
    FOR EACH ROW EXECUTE FUNCTION advisory_budget_consumption_guard();

ALTER TABLE advisory_budget_consumptions ENABLE ROW LEVEL SECURITY;
ALTER TABLE advisory_budget_consumptions FORCE ROW LEVEL SECURITY;
CREATE POLICY advisory_budget_consumption_tenant_scope ON advisory_budget_consumptions
    USING (CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid='advisory_budget_consumptions'::regclass))
        OR tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid)
    WITH CHECK (CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid='advisory_budget_consumptions'::regclass))
        OR tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid);
REVOKE ALL PRIVILEGES ON TABLE advisory_budget_consumptions FROM PUBLIC;
REVOKE ALL PRIVILEGES ON FUNCTION advisory_budget_consumption_guard() FROM PUBLIC;

-- Preserve every existing reason/state pairing and add the terminal budget case.
ALTER TABLE advisory_opportunity
    DROP CONSTRAINT advisory_opportunity_reason_check,
    ADD CONSTRAINT advisory_opportunity_reason_check CHECK (primary_reason IN (
        'workspace_disabled','session_skip','request_skip',
        'deterministic_input_invalid','capability_unavailable','provider_unconfigured',
        'budget_policy_invalid','choice_set_not_applicable','matrix_evidence_unresolved',
        'matrix_source_unverified','configuration_changed','matrix_task_revision_changed',
        'matrix_verification_stale','dispatch_authorized','recommendation_prepared',
        'provider_response','provider_failure','send_unknown',
        'budget_exhausted_after_response'
    )) NOT VALID;
ALTER TABLE advisory_opportunity
    DROP CONSTRAINT advisory_opportunity_state_reason_check,
    ADD CONSTRAINT advisory_opportunity_state_reason_check CHECK (
        (state='no_call' AND primary_reason IN (
            'workspace_disabled','session_skip','request_skip','deterministic_input_invalid',
            'capability_unavailable','provider_unconfigured','budget_policy_invalid',
            'choice_set_not_applicable'))
        OR (state='no_call' AND primary_reason IN
            ('matrix_evidence_unresolved','matrix_source_unverified')
            AND capability='engineering_profile' AND work_item_kind='matrix_task')
        OR (state='prepared' AND primary_reason='dispatch_authorized')
        OR (state='prepared' AND primary_reason='recommendation_prepared'
            AND capability='pipeline_recommendation'
            AND decision_point='pipeline_recommendation_before_slice_open')
        OR (state='awaiting_response' AND primary_reason IN ('dispatch_authorized','send_unknown'))
        OR (state='advised' AND primary_reason='provider_response')
        OR (state='invalidated' AND primary_reason='configuration_changed')
        OR (state='invalidated' AND primary_reason IN
            ('matrix_task_revision_changed','matrix_verification_stale')
            AND capability='engineering_profile' AND work_item_kind='matrix_task')
        OR (state='failed' AND primary_reason IN
            ('provider_failure','budget_exhausted_after_response'))
        OR (state='unresolved' AND primary_reason='send_unknown')
    ) NOT VALID;
