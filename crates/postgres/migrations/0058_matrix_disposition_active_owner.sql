-- The runtime role cannot lock private identity tables. Recheck the agent's
-- authorization inside the INSERT under the same database transaction, then
-- hold these rows against revocation or membership removal until commit.
CREATE FUNCTION matrix_disposition_require_active_owner() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, public
AS $active_owner$
DECLARE authorized boolean;
BEGIN
    IF NEW.tenant_id IS DISTINCT FROM
        NULLIF(pg_catalog.current_setting('tect.tenant_id', true), '')::uuid THEN
        RAISE EXCEPTION 'matrix disposition tenant does not match session'
            USING ERRCODE = '42501';
    END IF;

    SELECT true INTO authorized
    FROM public.agent_sessions AS s
    JOIN public.hosts AS h
        ON h.tenant_id = s.tenant_id AND h.id = s.host_id
    JOIN public.principals AS p
        ON p.tenant_id = h.tenant_id AND p.id = h.principal_id
    JOIN public.memberships AS m
        ON m.tenant_id = s.tenant_id AND m.workspace_id = s.workspace_id
        AND m.principal_id = p.id
    WHERE s.tenant_id = NEW.tenant_id
        AND s.workspace_id = NEW.workspace_id
        AND s.id = NEW.session_id
        AND h.principal_id = NEW.actor_id
        AND NOT s.revoked AND NOT h.revoked AND p.role = 'owner'
    FOR SHARE OF s, h, p, m;

    IF authorized IS DISTINCT FROM true THEN
        RAISE EXCEPTION 'matrix disposition requires an active workspace owner session'
            USING ERRCODE = '42501';
    END IF;
    RETURN NEW;
END
$active_owner$;

CREATE TRIGGER advisory_matrix_disposition_active_owner
    BEFORE INSERT ON advisory_matrix_disposition
    FOR EACH ROW EXECUTE FUNCTION matrix_disposition_require_active_owner();

REVOKE ALL PRIVILEGES ON FUNCTION matrix_disposition_require_active_owner() FROM PUBLIC;
