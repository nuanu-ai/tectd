ALTER TABLE principals DROP CONSTRAINT principals_role_check;
ALTER TABLE principals ADD CONSTRAINT principals_role_check
    CHECK (role IN ('owner', 'verifier'));

DROP FUNCTION public.tect_authenticate_host(uuid, text, boolean);

CREATE FUNCTION public.tect_authenticate_host(
    p_host_id uuid,
    p_credential_digest text,
    p_for_write boolean
)
RETURNS TABLE (
    tenant_id uuid,
    principal_id uuid,
    principal_role text,
    allowed_source_roots jsonb,
    allowed_setup_roots jsonb
)
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $function$
BEGIN
    IF p_for_write THEN
        RETURN QUERY
        SELECT h.tenant_id, h.principal_id, p.role, h.allowed_source_roots, h.allowed_setup_roots
        FROM public.hosts AS h
        JOIN public.principals AS p ON p.tenant_id = h.tenant_id AND p.id = h.principal_id
        WHERE h.id = p_host_id
          AND h.credential_digest = p_credential_digest
          AND NOT h.revoked
        FOR SHARE OF h, p;
    ELSE
        RETURN QUERY
        SELECT h.tenant_id, h.principal_id, p.role, h.allowed_source_roots, h.allowed_setup_roots
        FROM public.hosts AS h
        JOIN public.principals AS p ON p.tenant_id = h.tenant_id AND p.id = h.principal_id
        WHERE h.id = p_host_id
          AND h.credential_digest = p_credential_digest
          AND NOT h.revoked;
    END IF;
END
$function$;

REVOKE ALL PRIVILEGES ON FUNCTION public.tect_authenticate_host(uuid, text, boolean) FROM PUBLIC;
