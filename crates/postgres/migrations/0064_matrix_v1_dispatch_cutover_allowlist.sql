-- Capture exact V1 Matrix responses that were already sealed before this
-- migration. This proves database presence at cutover, not an external call.
-- Drain transactions that touched dispatch before taking the snapshot. An
-- operator must also stop old V1 senders before production cutover: a call
-- already outside PostgreSQL cannot be recalled by any table lock.
LOCK TABLE public.advisory_dispatch IN ACCESS EXCLUSIVE MODE;
LOCK TABLE public.advisory_opportunity, public.advisory_provider_observations,
    public.matrix_verifications, public.matrix_task_revisions,
    public.advisory_matrix_advice IN SHARE ROW EXCLUSIVE MODE;

CREATE FUNCTION public.matrix_v1_cutover_fingerprint(
    p_tenant uuid, p_workspace uuid, p_opportunity uuid, p_dispatch uuid
) RETURNS text LANGUAGE sql STABLE SECURITY INVOKER
SET search_path=pg_catalog,public AS $fingerprint$
    SELECT pg_catalog.encode(pg_catalog.sha256(pg_catalog.convert_to(
        pg_catalog.jsonb_build_array(
            o.tenant_id,o.workspace_id,o.id,o.work_item_id,o.matrix_task_revision,
            o.matrix_choice_set_digest,o.matrix_verification_digest,o.source_revision,
            o.session_id,o.authorized_actor_id,o.config_revision,o.session_preference,
            o.request_preference,o.policy_version,o.request_key,o.material_digest,
            EXTRACT(EPOCH FROM o.created_at),
            r.input_digest,r.choice_set_digest,
            v.id,v.schema,v.input_digest,v.owner_principal_id,v.verifier_principal_id,
            v.policy_version,v.record_digest,v.verification_reason,
            EXTRACT(EPOCH FROM v.verified_at),
            d.id,d.attempt_number,d.predecessor_dispatch_id,d.provider,d.model,
            d.configuration_snapshot,d.configuration_digest,d.material_digest,
            d.payload_digest,pg_catalog.octet_length(d.request_payload),
            pg_catalog.encode(pg_catalog.sha256(d.request_payload),'hex'),
            pg_catalog.octet_length(d.response_payload),
            pg_catalog.encode(pg_catalog.sha256(d.response_payload),'hex'),
            d.input_tokens,d.output_tokens,d.latency_ms,d.state,d.send_certainty,
            d.outcome,d.retry_basis,d.raw_response_ref,
            EXTRACT(EPOCH FROM d.authorized_at),
            EXTRACT(EPOCH FROM d.send_started_at),
            EXTRACT(EPOCH FROM d.sealed_at),
            p.configuration_digest,p.request_sha256,p.response_sha256,
            p.original_transport_outcome,p.response_complete,p.http_status,
            p.original_input_tokens,p.original_output_tokens,p.elapsed_ms,
            p.original_transport_context,
            EXTRACT(EPOCH FROM p.sealed_at),
            pg_catalog.octet_length(p.response_payload),
            pg_catalog.encode(pg_catalog.sha256(p.response_payload),'hex')
        )::text,'UTF8')),'hex')
    FROM public.advisory_opportunity o
    JOIN public.advisory_dispatch d
      ON (d.tenant_id,d.workspace_id,d.opportunity_id)=
         (o.tenant_id,o.workspace_id,o.id)
    JOIN public.matrix_task_revisions r
      ON (r.tenant_id,r.workspace_id,r.task_id,r.revision)=
         (o.tenant_id,o.workspace_id,o.work_item_id,o.matrix_task_revision)
    JOIN public.matrix_verifications v
      ON (v.tenant_id,v.workspace_id,v.task_id,v.task_revision,v.record_digest)=
         (o.tenant_id,o.workspace_id,o.work_item_id,o.matrix_task_revision,
          o.matrix_verification_digest)
    JOIN public.advisory_provider_observations p
      ON (p.tenant_id,p.workspace_id,p.opportunity_id,p.dispatch_id)=
         (d.tenant_id,d.workspace_id,d.opportunity_id,d.id)
    WHERE (o.tenant_id,o.workspace_id,o.id,d.id)=
          (p_tenant,p_workspace,p_opportunity,p_dispatch)
      AND (o.capability,o.decision_point,o.work_item_kind)=
          ('engineering_profile','engineering.profile.before_selection','matrix_task')
      AND o.matrix_choice_set_digest=r.choice_set_digest
      AND o.material_digest=d.material_digest
      AND v.schema='tect.matrix-verification/1'
      AND v.verification_reason='matrix_facts_verified'
      AND v.input_digest=r.input_digest
      AND d.state='sealed' AND d.send_certainty='sent'
      AND d.outcome='provider_response'
      AND d.send_started_at IS NOT NULL AND d.sealed_at IS NOT NULL
      AND d.response_payload IS NOT NULL
      AND d.payload_digest=pg_catalog.encode(pg_catalog.sha256(d.request_payload),'hex')
      AND p.original_transport_outcome='received' AND p.response_complete
      AND (p.http_status IS NULL OR p.http_status BETWEEN 200 AND 299)
      AND p.configuration_digest=d.configuration_digest
      AND p.request_sha256=d.payload_digest
      AND p.response_payload=d.response_payload
      AND p.response_sha256=pg_catalog.encode(pg_catalog.sha256(d.response_payload),'hex');
$fingerprint$;

CREATE TABLE public.matrix_v1_dispatch_cutover_allowlist (
    tenant_id uuid NOT NULL,
    workspace_id uuid NOT NULL,
    opportunity_id uuid NOT NULL,
    dispatch_id uuid NOT NULL,
    task_id uuid NOT NULL,
    task_revision bigint NOT NULL,
    verification_digest text NOT NULL CHECK (verification_digest ~ '^[0-9a-f]{64}$'),
    fingerprint text NOT NULL CHECK (fingerprint ~ '^[0-9a-f]{64}$'),
    captured_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
    PRIMARY KEY (tenant_id,workspace_id,opportunity_id,dispatch_id),
    FOREIGN KEY (tenant_id,workspace_id,opportunity_id,dispatch_id)
        REFERENCES public.advisory_dispatch (tenant_id,workspace_id,opportunity_id,id),
    FOREIGN KEY (tenant_id,workspace_id,task_id,task_revision,verification_digest)
        REFERENCES public.matrix_verifications
            (tenant_id,workspace_id,task_id,task_revision,record_digest)
);

INSERT INTO public.matrix_v1_dispatch_cutover_allowlist
    (tenant_id,workspace_id,opportunity_id,dispatch_id,task_id,task_revision,
     verification_digest,fingerprint)
SELECT o.tenant_id,o.workspace_id,o.id,d.id,o.work_item_id,
       o.matrix_task_revision,o.matrix_verification_digest,f.fingerprint
FROM public.advisory_opportunity o
JOIN public.advisory_dispatch d
  ON (d.tenant_id,d.workspace_id,d.opportunity_id)=
     (o.tenant_id,o.workspace_id,o.id)
CROSS JOIN LATERAL (
    SELECT public.matrix_v1_cutover_fingerprint(
        o.tenant_id,o.workspace_id,o.id,d.id) AS fingerprint
) f
WHERE (o.capability,o.decision_point,o.work_item_kind)=
      ('engineering_profile','engineering.profile.before_selection','matrix_task')
  AND o.state IN ('awaiting_response','unresolved','advised')
  AND o.matrix_verification_digest IS NOT NULL
  AND f.fingerprint IS NOT NULL
  AND NOT EXISTS (
      SELECT 1 FROM public.advisory_matrix_advice a
      WHERE (a.tenant_id,a.workspace_id,a.opportunity_id)=
            (o.tenant_id,o.workspace_id,o.id));

CREATE FUNCTION public.matrix_v1_cutover_allowlist_immutable() RETURNS trigger
LANGUAGE plpgsql SECURITY INVOKER SET search_path=pg_catalog,public AS $guard$
BEGIN
    RAISE EXCEPTION 'Matrix V1 cutover allowlist is immutable' USING ERRCODE='23514';
END $guard$;
CREATE TRIGGER matrix_v1_cutover_allowlist_immutable
    BEFORE INSERT OR UPDATE OR DELETE ON public.matrix_v1_dispatch_cutover_allowlist
    FOR EACH ROW EXECUTE FUNCTION public.matrix_v1_cutover_allowlist_immutable();

ALTER TABLE public.matrix_v1_dispatch_cutover_allowlist ENABLE ROW LEVEL SECURITY;
ALTER TABLE public.matrix_v1_dispatch_cutover_allowlist FORCE ROW LEVEL SECURITY;
CREATE POLICY matrix_v1_cutover_allowlist_tenant_read
    ON public.matrix_v1_dispatch_cutover_allowlist FOR SELECT
    USING (tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid);
REVOKE ALL PRIVILEGES ON TABLE public.matrix_v1_dispatch_cutover_allowlist FROM PUBLIC;
REVOKE ALL PRIVILEGES ON FUNCTION public.matrix_v1_cutover_allowlist_immutable() FROM PUBLIC;

-- Runtime has direct INSERT permission on advice. Close that path as well as
-- the application store path, without changing V2 or pre-existing receipts.
CREATE FUNCTION public.matrix_v1_cutover_advice_insert_guard() RETURNS trigger
LANGUAGE plpgsql SECURITY INVOKER SET search_path=pg_catalog,public AS $guard$
DECLARE
    opportunity_record record;
    verification_schema text;
    dispatch_record record;
    captured_fingerprint text;
BEGIN
    SELECT o.capability,o.decision_point,o.work_item_kind,o.work_item_id,
           o.matrix_task_revision,o.matrix_choice_set_digest,o.matrix_verification_digest
      INTO opportunity_record
      FROM public.advisory_opportunity o
     WHERE (o.tenant_id,o.workspace_id,o.id)=
           (NEW.tenant_id,NEW.workspace_id,NEW.opportunity_id)
     FOR UPDATE;
    IF NOT FOUND THEN
        RETURN NEW;
    END IF;
    IF (opportunity_record.capability,opportunity_record.decision_point,
        opportunity_record.work_item_kind) IS DISTINCT FROM
       ('engineering_profile','engineering.profile.before_selection','matrix_task') THEN
        RETURN NEW;
    END IF;
    SELECT v.schema INTO verification_schema
      FROM public.matrix_verifications v
     WHERE (v.tenant_id,v.workspace_id,v.task_id,v.task_revision,v.record_digest)=
           (NEW.tenant_id,NEW.workspace_id,opportunity_record.work_item_id,
            opportunity_record.matrix_task_revision,
            opportunity_record.matrix_verification_digest);
    IF verification_schema='tect.context-matrix-verification/1' THEN
        RETURN NEW;
    END IF;
    IF verification_schema IS DISTINCT FROM 'tect.matrix-verification/1' THEN
        RAISE EXCEPTION 'Matrix advice requires a versioned verification'
            USING ERRCODE='23514';
    END IF;
    SELECT d.id,d.state,d.send_certainty,d.outcome,d.response_payload
      INTO dispatch_record
      FROM public.advisory_dispatch d
     WHERE (d.tenant_id,d.workspace_id,d.opportunity_id,d.id)=
           (NEW.tenant_id,NEW.workspace_id,NEW.opportunity_id,NEW.dispatch_id)
     FOR UPDATE;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'Matrix V1 advice requires exact pre-cutover sealed response'
            USING ERRCODE='23514';
    END IF;
    IF dispatch_record.state IS DISTINCT FROM 'sealed'
       OR dispatch_record.send_certainty IS DISTINCT FROM 'sent'
       OR dispatch_record.outcome IS DISTINCT FROM 'provider_response'
       OR dispatch_record.response_payload IS NULL
       OR NEW.response_payload_sha256 IS DISTINCT FROM
          pg_catalog.encode(pg_catalog.sha256(dispatch_record.response_payload),'hex')
       OR (NEW.task_id,NEW.matrix_task_revision,NEW.matrix_choice_set_digest) IS DISTINCT FROM
          (opportunity_record.work_item_id,opportunity_record.matrix_task_revision,
           opportunity_record.matrix_choice_set_digest)
    THEN
        RAISE EXCEPTION 'Matrix V1 advice requires exact pre-cutover sealed response'
            USING ERRCODE='23514';
    END IF;
    SELECT a.fingerprint INTO captured_fingerprint
      FROM public.matrix_v1_dispatch_cutover_allowlist a
     WHERE (a.tenant_id,a.workspace_id,a.opportunity_id,a.dispatch_id,
            a.task_id,a.task_revision,a.verification_digest)=
           (NEW.tenant_id,NEW.workspace_id,NEW.opportunity_id,NEW.dispatch_id,
            NEW.task_id,NEW.matrix_task_revision,
            opportunity_record.matrix_verification_digest);
    IF captured_fingerprint IS NULL OR captured_fingerprint IS DISTINCT FROM
       public.matrix_v1_cutover_fingerprint(
           NEW.tenant_id,NEW.workspace_id,NEW.opportunity_id,NEW.dispatch_id)
    THEN
        RAISE EXCEPTION 'Matrix V1 advice dispatch was not sealed at cutover'
            USING ERRCODE='23514';
    END IF;
    RETURN NEW;
END $guard$;
CREATE TRIGGER matrix_v1_cutover_advice_insert_guard
    BEFORE INSERT ON public.advisory_matrix_advice FOR EACH ROW
    EXECUTE FUNCTION public.matrix_v1_cutover_advice_insert_guard();
REVOKE ALL PRIVILEGES ON FUNCTION public.matrix_v1_cutover_advice_insert_guard() FROM PUBLIC;
