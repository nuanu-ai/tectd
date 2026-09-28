-- Search-only pgRDF 0.6.34 adapter. Keep the 0041 reader's authorization and
-- subject-ownership boundary, but visit only selected subjects' indexed quads.
-- Rust still verifies every returned native triple against canonical RDF and
-- the publication receipt before a search result is accepted.
CREATE OR REPLACE FUNCTION public.tect_dk2_search_native_rows(
  p_tenant uuid, p_workspace uuid, p_principal uuid,
  p_units uuid[], p_revisions bigint[], p_events uuid[]
) RETURNS TABLE(unit_id uuid, triple jsonb)
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $fn$
DECLARE
  caller_is_admin boolean;
  graph_iri text;
  graph_key bigint;
  native_version text;
  native_build_id text;
  extension_version text;
  schema_compatible boolean;
BEGIN
  SELECT pg_catalog.pg_has_role(SESSION_USER,d.datdba,'MEMBER')
    INTO caller_is_admin
  FROM pg_catalog.pg_database d WHERE d.datname=pg_catalog.current_database();
  IF caller_is_admin IS DISTINCT FROM true
     AND public.tect_dk_database_identity_ready() IS DISTINCT FROM true THEN
    RAISE EXCEPTION USING ERRCODE='55000',MESSAGE='durable knowledge recovery required';
  END IF;
  IF NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid IS DISTINCT FROM p_tenant
     OR NOT EXISTS (SELECT 1 FROM public.workspace_knowledge_state s
                    WHERE s.tenant_id=p_tenant AND s.workspace_id=p_workspace AND s.capability_ready)
     OR p_units IS NULL OR p_revisions IS NULL OR p_events IS NULL
     OR pg_catalog.cardinality(p_units)<>pg_catalog.cardinality(p_revisions)
     OR pg_catalog.cardinality(p_units)<>pg_catalog.cardinality(p_events)
     OR pg_catalog.cardinality(p_units)>512
     OR pg_catalog.array_position(p_units,NULL) IS NOT NULL
     OR pg_catalog.array_position(p_revisions,NULL) IS NOT NULL
     OR pg_catalog.array_position(p_events,NULL) IS NOT NULL
     OR EXISTS (SELECT 1 FROM pg_catalog.unnest(p_revisions) AS r(revision) WHERE r.revision<1)
  THEN
    RAISE EXCEPTION USING ERRCODE='42501',MESSAGE='durable knowledge unavailable';
  END IF;
  IF pg_catalog.cardinality(p_units)=0 THEN RETURN; END IF;

  -- The private representation is an explicit pinned dependency. A changed
  -- engine or column shape must fail closed before returning partial results.
  IF pg_catalog.to_regnamespace('pgrdf') IS NULL THEN
    RAISE EXCEPTION USING ERRCODE='55000',MESSAGE='unsupported durable knowledge search engine';
  END IF;
  BEGIN
    EXECUTE 'SELECT pgrdf.version(),pgrdf.build_id()' INTO native_version,native_build_id;
    SELECT extversion INTO extension_version
    FROM pg_catalog.pg_extension WHERE extname='pgrdf';
    WITH expected(relation_name,column_name,type_name) AS (
      VALUES
        ('_pgrdf_dictionary','id','int8'),
        ('_pgrdf_dictionary','term_type','int2'),
        ('_pgrdf_dictionary','lexical_value','text'),
        ('_pgrdf_dictionary','datatype_iri_id','int8'),
        ('_pgrdf_dictionary','language_tag','text'),
        ('_pgrdf_quads','subject_id','int8'),
        ('_pgrdf_quads','predicate_id','int8'),
        ('_pgrdf_quads','object_id','int8'),
        ('_pgrdf_quads','graph_id','int8'),
        ('_pgrdf_quads','is_inferred','bool'),
        ('_pgrdf_graphs','graph_id','int8'),
        ('_pgrdf_graphs','iri','text')
    )
    SELECT count(*)=12 INTO schema_compatible
    FROM expected e
    JOIN pg_catalog.pg_namespace n ON n.nspname='pgrdf'
    JOIN pg_catalog.pg_class c ON c.relnamespace=n.oid AND c.relname=e.relation_name
    JOIN pg_catalog.pg_attribute a ON a.attrelid=c.oid AND a.attname=e.column_name
      AND NOT a.attisdropped
    JOIN pg_catalog.pg_type t ON t.oid=a.atttypid AND t.typname=e.type_name;
  EXCEPTION WHEN undefined_function OR undefined_table OR undefined_column THEN
    RAISE EXCEPTION USING ERRCODE='55000',MESSAGE='unsupported durable knowledge search engine';
  END;
  IF native_version IS DISTINCT FROM '0.6.34'
     OR native_build_id IS DISTINCT FROM 'v0.6.34'
     OR extension_version IS DISTINCT FROM '0.6.34'
     OR schema_compatible IS DISTINCT FROM true
     OR pg_catalog.to_regprocedure('pgrdf.graph_id(text)') IS NULL THEN
    RAISE EXCEPTION USING ERRCODE='55000',MESSAGE='unsupported durable knowledge search engine';
  END IF;

  graph_iri := 'urn:tect:dk:workspace:'||p_tenant||':'||p_workspace;
  EXECUTE 'SELECT pgrdf.graph_id($1)' INTO graph_key USING graph_iri;
  IF graph_key IS NULL THEN
    RAISE EXCEPTION USING ERRCODE='55000',MESSAGE='durable knowledge stable graph unavailable';
  END IF;

  RETURN QUERY
  WITH candidates AS MATERIALIZED (
    SELECT c.unit_id,c.unit_id::text AS unit_key,c.event_id::text AS event_key,
      'urn:tect:dk:unit:'||p_tenant||':'||p_workspace||':'||c.unit_id AS ui,
      'urn:tect:dk:unit:'||p_tenant||':'||p_workspace||':'||c.unit_id||':revision:'||c.revision AS ri,
      'urn:tect:dk:event:'||p_tenant||':'||p_workspace||':'||c.event_id AS ei,
      'urn:tect:dk:unit:'||p_tenant||':'||p_workspace||':'||c.unit_id||':event:'||c.event_id AS ep
    FROM (SELECT p_units[i] AS unit_id,p_revisions[i] AS revision,p_events[i] AS event_id
          FROM pg_catalog.generate_subscripts(p_units,1) AS i) c
    JOIN public.knowledge_unit_heads h ON h.tenant_id=p_tenant AND h.workspace_id=p_workspace
      AND h.unit_id=c.unit_id AND h.accepted_revision=c.revision AND h.lifecycle='active'
      AND h.active AND NOT h.payload_erased
    JOIN public.knowledge_revisions rev ON rev.tenant_id=h.tenant_id AND rev.workspace_id=h.workspace_id
      AND rev.unit_id=h.unit_id AND rev.revision=c.revision AND rev.publication_event_id=c.event_id
      AND NOT rev.payload_erased
    JOIN public.knowledge_search_resources r ON r.tenant_id=h.tenant_id AND r.workspace_id=h.workspace_id
      AND r.unit_id=h.unit_id AND r.revision=c.revision
    WHERE NOT EXISTS (SELECT 1 FROM public.knowledge_suppression_ledger l
                      WHERE l.tenant_id=h.tenant_id AND l.workspace_id=h.workspace_id AND l.unit_id=h.unit_id)
      AND ((h.access_scope='workspace_members' AND rev.access_scope='workspace_members'
            AND r.access_scope='workspace_members') OR public.tect_dk_is_owner(p_principal))
  ), selected_subjects AS MATERIALIZED (
    SELECT c.unit_id,s.id AS subject_id
    FROM pgrdf._pgrdf_dictionary s JOIN candidates c ON
      ((pg_catalog.split_part(s.lexical_value,':',4)='unit'
        AND pg_catalog.split_part(s.lexical_value,':',7)=c.unit_key
        AND ((s.lexical_value=c.ui) OR s.lexical_value=c.ri
          OR pg_catalog.left(s.lexical_value,pg_catalog.length(c.ri)+1)=c.ri||':'
          OR s.lexical_value=c.ep
          OR pg_catalog.left(s.lexical_value,pg_catalog.length(c.ep)+1)=c.ep||':'))
       OR (pg_catalog.split_part(s.lexical_value,':',4)='event'
        AND pg_catalog.split_part(s.lexical_value,':',7)=c.event_key
        AND s.lexical_value=c.ei))
  ), selected_quads AS MATERIALIZED (
    SELECT ss.unit_id,q.subject_id,q.predicate_id,q.object_id
    FROM selected_subjects ss
    JOIN pgrdf._pgrdf_quads q ON q.subject_id=ss.subject_id AND q.graph_id=graph_key
  )
  SELECT q.unit_id,
    pg_catalog.jsonb_build_object(
      'subject',pg_catalog.jsonb_build_object('type',CASE s.term_type WHEN 1 THEN 'iri' WHEN 2 THEN 'bnode' WHEN 3 THEN 'literal' ELSE 'unsupported' END,'value',s.lexical_value),
      'predicate',pg_catalog.jsonb_build_object('type',CASE p.term_type WHEN 1 THEN 'iri' WHEN 2 THEN 'bnode' WHEN 3 THEN 'literal' ELSE 'unsupported' END,'value',p.lexical_value),
      'object',CASE WHEN o.term_type=1 THEN
        pg_catalog.jsonb_build_object('type','iri','value',o.lexical_value)
      WHEN o.term_type=2 THEN
        pg_catalog.jsonb_build_object('type','bnode','value',o.lexical_value)
      WHEN o.term_type=3 THEN
        pg_catalog.jsonb_build_object('type','literal','value',o.lexical_value,
          'datatype',CASE WHEN o.language_tag IS NOT NULL THEN
            'http://www.w3.org/1999/02/22-rdf-syntax-ns#langString'
          ELSE COALESCE(dt.lexical_value,'http://www.w3.org/2001/XMLSchema#string') END)
        || CASE WHEN o.language_tag IS NOT NULL THEN
          pg_catalog.jsonb_build_object('language',o.language_tag)
        ELSE '{}'::jsonb END
      ELSE pg_catalog.jsonb_build_object('type','unsupported','value',o.lexical_value)
      END) AS triple
  FROM selected_quads q
  JOIN pgrdf._pgrdf_dictionary s ON s.id=q.subject_id
  JOIN pgrdf._pgrdf_dictionary p ON p.id=q.predicate_id
  JOIN pgrdf._pgrdf_dictionary o ON o.id=q.object_id
  LEFT JOIN pgrdf._pgrdf_dictionary dt ON dt.id=o.datatype_iri_id
  JOIN candidates c ON c.unit_id=q.unit_id
  WHERE (s.lexical_value<>c.ui OR
         (p.lexical_value='http://www.w3.org/1999/02/22-rdf-syntax-ns#type'
          AND o.lexical_value='urn:tect:dk:KnowledgeUnit') OR
         (p.lexical_value='urn:tect:dk:hasRevision' AND o.lexical_value=c.ri));
END
$fn$;

REVOKE ALL PRIVILEGES ON FUNCTION public.tect_dk2_search_native_rows(
  uuid,uuid,uuid,uuid[],bigint[],uuid[]) FROM PUBLIC;
