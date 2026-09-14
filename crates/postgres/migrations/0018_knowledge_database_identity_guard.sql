-- Bind effective durable-knowledge readiness to this exact PostgreSQL database.
-- Managed recovery invokes only the owner-gated erase/residual wrappers while a
-- restored database still has a deliberately mismatched qualification identity.
CREATE FUNCTION public.tect_dk_database_identity_ready()
RETURNS boolean
LANGUAGE sql
STABLE
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $fn$
  SELECT COALESCE(
    c.capability_ready
    AND c.qualified_system_identifier IS NOT NULL
    AND c.qualified_database_oid IS NOT NULL
    AND c.qualified_system_identifier = s.system_identifier::text
    AND c.qualified_database_oid = d.oid,
    false
  )
  FROM public.durable_knowledge_capability c
  CROSS JOIN pg_catalog.pg_control_system() s
  JOIN pg_catalog.pg_database d ON d.datname=pg_catalog.current_database()
  WHERE c.singleton
$fn$;

REVOKE ALL PRIVILEGES ON FUNCTION public.tect_dk_database_identity_ready() FROM PUBLIC;

-- Preserve the already qualified native implementations behind owner-only
-- names. The stable public signatures below are the only runtime surfaces.
ALTER FUNCTION public.tect_dk_native_publish(uuid,uuid,uuid,text,text,text)
  RENAME TO tect_dk_internal_native_publish;
ALTER FUNCTION public.tect_dk_native_read(uuid,uuid,uuid,bigint,uuid)
  RENAME TO tect_dk_internal_native_read;
ALTER FUNCTION public.tect_dk_native_erase(uuid,uuid,uuid)
  RENAME TO tect_dk_internal_native_erase;
ALTER FUNCTION public.tect_dk_native_owned_residual(uuid,uuid,uuid)
  RENAME TO tect_dk_internal_native_owned_residual;
ALTER FUNCTION public.tect_dk2_native_publish(uuid,uuid,uuid,text,text,text)
  RENAME TO tect_dk2_internal_native_publish;
ALTER FUNCTION public.tect_dk2_native_read(uuid,uuid,uuid,bigint,uuid,boolean)
  RENAME TO tect_dk2_internal_native_read;
ALTER FUNCTION public.tect_dk_capability()
  RENAME TO tect_dk_internal_capability;

REVOKE ALL PRIVILEGES ON FUNCTION
  public.tect_dk_internal_native_publish(uuid,uuid,uuid,text,text,text),
  public.tect_dk_internal_native_read(uuid,uuid,uuid,bigint,uuid),
  public.tect_dk_internal_native_erase(uuid,uuid,uuid),
  public.tect_dk_internal_native_owned_residual(uuid,uuid,uuid),
  public.tect_dk2_internal_native_publish(uuid,uuid,uuid,text,text,text),
  public.tect_dk2_internal_native_read(uuid,uuid,uuid,bigint,uuid,boolean),
  public.tect_dk_internal_capability()
FROM PUBLIC;

-- Remove explicit EXECUTE ACLs retained by ALTER FUNCTION on an upgraded DB.
DO $block$
DECLARE grantee_name text;
BEGIN
  FOR grantee_name IN
    SELECT DISTINCT role.rolname
    FROM pg_catalog.pg_proc procedure
    CROSS JOIN LATERAL pg_catalog.aclexplode(
      COALESCE(procedure.proacl,pg_catalog.acldefault('f',procedure.proowner))
    ) acl
    JOIN pg_catalog.pg_roles role ON role.oid=acl.grantee
    WHERE procedure.oid = ANY(ARRAY[
      pg_catalog.to_regprocedure('public.tect_dk_internal_native_publish(uuid,uuid,uuid,text,text,text)'),
      pg_catalog.to_regprocedure('public.tect_dk_internal_native_read(uuid,uuid,uuid,bigint,uuid)'),
      pg_catalog.to_regprocedure('public.tect_dk_internal_native_erase(uuid,uuid,uuid)'),
      pg_catalog.to_regprocedure('public.tect_dk_internal_native_owned_residual(uuid,uuid,uuid)'),
      pg_catalog.to_regprocedure('public.tect_dk2_internal_native_publish(uuid,uuid,uuid,text,text,text)'),
      pg_catalog.to_regprocedure('public.tect_dk2_internal_native_read(uuid,uuid,uuid,bigint,uuid,boolean)'),
      pg_catalog.to_regprocedure('public.tect_dk_internal_capability()')
    ]::oid[])
      AND acl.privilege_type='EXECUTE'
      AND acl.grantee<>procedure.proowner
  LOOP
    EXECUTE pg_catalog.format(
      'REVOKE ALL PRIVILEGES ON FUNCTION public.tect_dk_internal_native_publish(uuid,uuid,uuid,text,text,text),public.tect_dk_internal_native_read(uuid,uuid,uuid,bigint,uuid),public.tect_dk_internal_native_erase(uuid,uuid,uuid),public.tect_dk_internal_native_owned_residual(uuid,uuid,uuid),public.tect_dk2_internal_native_publish(uuid,uuid,uuid,text,text,text),public.tect_dk2_internal_native_read(uuid,uuid,uuid,bigint,uuid,boolean),public.tect_dk_internal_capability() FROM %I',
      grantee_name
    );
  END LOOP;
END
$block$;

CREATE FUNCTION public.tect_dk_native_publish(
  p_tenant uuid,p_workspace uuid,p_event uuid,p_operation text,
  p_payload text,p_stable_payload text
) RETURNS text
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public
AS $fn$
DECLARE caller_is_admin boolean;
BEGIN
  SELECT pg_catalog.pg_has_role(SESSION_USER,d.datdba,'MEMBER')
    INTO caller_is_admin
  FROM pg_catalog.pg_database d WHERE d.datname=pg_catalog.current_database();
  IF caller_is_admin IS DISTINCT FROM true
     AND public.tect_dk_database_identity_ready() IS DISTINCT FROM true THEN
    RAISE EXCEPTION USING ERRCODE='55000',MESSAGE='durable knowledge recovery required';
  END IF;
  RETURN public.tect_dk_internal_native_publish(
    p_tenant,p_workspace,p_event,p_operation,p_payload,p_stable_payload
  );
END
$fn$;

CREATE FUNCTION public.tect_dk_native_read(
  p_tenant uuid,p_workspace uuid,p_unit uuid,p_revision bigint,p_event uuid
) RETURNS SETOF jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public
AS $fn$
DECLARE caller_is_admin boolean;
BEGIN
  SELECT pg_catalog.pg_has_role(SESSION_USER,d.datdba,'MEMBER')
    INTO caller_is_admin
  FROM pg_catalog.pg_database d WHERE d.datname=pg_catalog.current_database();
  IF caller_is_admin IS DISTINCT FROM true
     AND public.tect_dk_database_identity_ready() IS DISTINCT FROM true THEN
    RAISE EXCEPTION USING ERRCODE='55000',MESSAGE='durable knowledge recovery required';
  END IF;
  RETURN QUERY SELECT * FROM public.tect_dk_internal_native_read(
    p_tenant,p_workspace,p_unit,p_revision,p_event
  );
END
$fn$;

CREATE FUNCTION public.tect_dk2_native_publish(
  p_tenant uuid,p_workspace uuid,p_event uuid,p_operation text,
  p_payload text,p_stable_payload text
) RETURNS text
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public
AS $fn$
DECLARE caller_is_admin boolean;
BEGIN
  SELECT pg_catalog.pg_has_role(SESSION_USER,d.datdba,'MEMBER')
    INTO caller_is_admin
  FROM pg_catalog.pg_database d WHERE d.datname=pg_catalog.current_database();
  IF caller_is_admin IS DISTINCT FROM true
     AND public.tect_dk_database_identity_ready() IS DISTINCT FROM true THEN
    RAISE EXCEPTION USING ERRCODE='55000',MESSAGE='durable knowledge recovery required';
  END IF;
  RETURN public.tect_dk2_internal_native_publish(
    p_tenant,p_workspace,p_event,p_operation,p_payload,p_stable_payload
  );
END
$fn$;

CREATE FUNCTION public.tect_dk2_native_read(
  p_tenant uuid,p_workspace uuid,p_unit uuid,p_revision bigint,
  p_event uuid,p_include_revision boolean
) RETURNS SETOF jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public
AS $fn$
DECLARE caller_is_admin boolean;
BEGIN
  SELECT pg_catalog.pg_has_role(SESSION_USER,d.datdba,'MEMBER')
    INTO caller_is_admin
  FROM pg_catalog.pg_database d WHERE d.datname=pg_catalog.current_database();
  IF caller_is_admin IS DISTINCT FROM true
     AND public.tect_dk_database_identity_ready() IS DISTINCT FROM true THEN
    RAISE EXCEPTION USING ERRCODE='55000',MESSAGE='durable knowledge recovery required';
  END IF;
  RETURN QUERY SELECT * FROM public.tect_dk2_internal_native_read(
    p_tenant,p_workspace,p_unit,p_revision,p_event,p_include_revision
  );
END
$fn$;

CREATE FUNCTION public.tect_dk_native_erase(
  p_tenant uuid,p_workspace uuid,p_unit uuid
) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public
AS $fn$
DECLARE caller_is_admin boolean;
BEGIN
  SELECT pg_catalog.pg_has_role(SESSION_USER,d.datdba,'MEMBER')
    INTO caller_is_admin
  FROM pg_catalog.pg_database d WHERE d.datname=pg_catalog.current_database();
  IF caller_is_admin IS DISTINCT FROM true
     AND public.tect_dk_database_identity_ready() IS DISTINCT FROM true THEN
    RAISE EXCEPTION USING ERRCODE='55000',MESSAGE='durable knowledge recovery required';
  END IF;
  RETURN public.tect_dk_internal_native_erase(p_tenant,p_workspace,p_unit);
END
$fn$;

CREATE FUNCTION public.tect_dk_native_owned_residual(
  p_tenant uuid,p_workspace uuid,p_unit uuid
) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public
AS $fn$
DECLARE caller_is_admin boolean;
BEGIN
  SELECT pg_catalog.pg_has_role(SESSION_USER,d.datdba,'MEMBER')
    INTO caller_is_admin
  FROM pg_catalog.pg_database d WHERE d.datname=pg_catalog.current_database();
  IF caller_is_admin IS DISTINCT FROM true
     AND public.tect_dk_database_identity_ready() IS DISTINCT FROM true THEN
    RAISE EXCEPTION USING ERRCODE='55000',MESSAGE='durable knowledge recovery required';
  END IF;
  RETURN public.tect_dk_internal_native_owned_residual(p_tenant,p_workspace,p_unit);
END
$fn$;

CREATE FUNCTION public.tect_dk_capability()
RETURNS TABLE(capability_ready boolean,pgrdf_version text)
LANGUAGE sql STABLE SECURITY DEFINER SET search_path=pg_catalog,public
AS $fn$
  SELECT raw.capability_ready
         AND public.tect_dk_database_identity_ready(),
         raw.pgrdf_version
  FROM public.tect_dk_internal_capability() raw
$fn$;

REVOKE ALL PRIVILEGES ON FUNCTION
  public.tect_dk_native_publish(uuid,uuid,uuid,text,text,text),
  public.tect_dk_native_read(uuid,uuid,uuid,bigint,uuid),
  public.tect_dk2_native_publish(uuid,uuid,uuid,text,text,text),
  public.tect_dk2_native_read(uuid,uuid,uuid,bigint,uuid,boolean),
  public.tect_dk_native_erase(uuid,uuid,uuid),
  public.tect_dk_native_owned_residual(uuid,uuid,uuid),
  public.tect_dk_capability()
FROM PUBLIC;
