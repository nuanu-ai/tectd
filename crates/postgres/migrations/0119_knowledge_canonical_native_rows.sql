-- Generic canonical pgRDF 0.6.34 acquisition, indexed by full input ordinal.
-- Preserve 0012 native_read exact subject ownership; do not import search-only
-- head/lifecycle/projection/suppression policy. Rust retains exact RDF and receipt
-- verification. Private native representation is pinned and asserted below.
CREATE FUNCTION public.tect_dk2_canonical_native_rows(
  p_tenant uuid, p_workspace uuid, p_principal uuid,
  p_units uuid[], p_revisions bigint[], p_events uuid[], p_include_revisions boolean[]
) RETURNS TABLE(input_ordinal bigint, triple jsonb)
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
     OR NOT EXISTS (SELECT 1 FROM public.workspaces w WHERE w.tenant_id=p_tenant AND w.id=p_workspace)
     OR NOT EXISTS (SELECT 1 FROM public.workspace_knowledge_state s
                    WHERE s.tenant_id=p_tenant AND s.workspace_id=p_workspace AND s.capability_ready)
     OR p_units IS NULL OR p_revisions IS NULL OR p_events IS NULL OR p_include_revisions IS NULL
     OR pg_catalog.cardinality(p_units)<>pg_catalog.cardinality(p_revisions)
     OR pg_catalog.cardinality(p_units)<>pg_catalog.cardinality(p_events)
     OR pg_catalog.cardinality(p_units)<>pg_catalog.cardinality(p_include_revisions)
     OR pg_catalog.cardinality(p_units)>512
     OR pg_catalog.array_position(p_units,NULL) IS NOT NULL
     OR pg_catalog.array_position(p_revisions,NULL) IS NOT NULL
     OR pg_catalog.array_position(p_events,NULL) IS NOT NULL
     OR pg_catalog.array_position(p_include_revisions,NULL) IS NOT NULL
     OR EXISTS (SELECT 1 FROM pg_catalog.unnest(p_revisions) AS r(revision) WHERE r.revision<1)
  THEN
    RAISE EXCEPTION USING ERRCODE='42501',MESSAGE='durable knowledge unavailable';
  END IF;
  IF pg_catalog.cardinality(p_units)=0 THEN RETURN; END IF;

  -- The principal comes from the authenticated native-session operation, as in
  -- the search adapter. Validate each requested identity, including historical
  -- revisions and lifecycle events, without adding document-nested ACL here.
  IF EXISTS (
    SELECT 1
    FROM ROWS FROM (pg_catalog.unnest(p_units),pg_catalog.unnest(p_revisions),
      pg_catalog.unnest(p_events),pg_catalog.unnest(p_include_revisions))
      WITH ORDINALITY AS c(unit_id,revision,event_id,include_revision,input_ordinal)
    LEFT JOIN public.knowledge_unit_heads h ON h.tenant_id=p_tenant
      AND h.workspace_id=p_workspace AND h.unit_id=c.unit_id
    LEFT JOIN public.knowledge_revisions rev ON rev.tenant_id=p_tenant
      AND rev.workspace_id=p_workspace AND rev.unit_id=c.unit_id AND rev.revision=c.revision
    LEFT JOIN public.knowledge_publication_events e ON e.tenant_id=p_tenant
      AND e.workspace_id=p_workspace AND e.id=c.event_id
      AND e.unit_id=c.unit_id AND e.unit_revision=c.revision
    WHERE h.unit_id IS NULL OR rev.unit_id IS NULL OR e.id IS NULL
      OR h.contract_version<>'dk-2' OR rev.contract_version<>'dk-2' OR e.contract_version<>'dk-2'
      OR h.payload_erased OR rev.payload_erased OR e.payload_erased
      OR h.lifecycle IN ('erased','erasure_pending')
      OR NOT ((h.access_scope='workspace_members' AND rev.access_scope='workspace_members')
              OR public.tect_dk_is_owner(p_principal))
  ) THEN
    RAISE EXCEPTION USING ERRCODE='42501',MESSAGE='durable knowledge unavailable';
  END IF;

  -- The private representation is an explicit pinned dependency. A changed
  -- engine or column shape must fail closed before returning partial results.
  IF pg_catalog.to_regnamespace('pgrdf') IS NULL THEN
    RAISE EXCEPTION USING ERRCODE='55000',MESSAGE='unsupported durable knowledge canonical engine';
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
    RAISE EXCEPTION USING ERRCODE='55000',MESSAGE='unsupported durable knowledge canonical engine';
  END;
  IF native_version IS DISTINCT FROM '0.6.34'
     OR native_build_id IS DISTINCT FROM 'v0.6.34'
     OR extension_version IS DISTINCT FROM '0.6.34'
     OR schema_compatible IS DISTINCT FROM true
     OR pg_catalog.to_regprocedure('pgrdf.graph_id(text)') IS NULL THEN
    RAISE EXCEPTION USING ERRCODE='55000',MESSAGE='unsupported durable knowledge canonical engine';
  END IF;

  graph_iri := 'urn:tect:dk:workspace:'||p_tenant||':'||p_workspace;
  EXECUTE 'SELECT pgrdf.graph_id($1)' INTO graph_key USING graph_iri;
  IF graph_key IS NULL THEN
    RAISE EXCEPTION USING ERRCODE='55000',MESSAGE='durable knowledge stable graph unavailable';
  END IF;

  RETURN QUERY
  WITH candidates AS MATERIALIZED (
    SELECT c.input_ordinal,c.include_revision,c.unit_id::text AS unit_key,c.event_id::text AS event_key,
      'urn:tect:dk:unit:'||p_tenant||':'||p_workspace||':'||c.unit_id AS ui,
      'urn:tect:dk:unit:'||p_tenant||':'||p_workspace||':'||c.unit_id||':revision:'||c.revision AS ri,
      'urn:tect:dk:event:'||p_tenant||':'||p_workspace||':'||c.event_id AS ei,
      'urn:tect:dk:unit:'||p_tenant||':'||p_workspace||':'||c.unit_id||':event:'||c.event_id AS ep
    FROM ROWS FROM (pg_catalog.unnest(p_units),pg_catalog.unnest(p_revisions),
      pg_catalog.unnest(p_events),pg_catalog.unnest(p_include_revisions))
      WITH ORDINALITY AS c(unit_id,revision,event_id,include_revision,input_ordinal)
  ), selected_subjects AS MATERIALIZED (
    SELECT c.input_ordinal,s.id AS subject_id
    FROM pgrdf._pgrdf_dictionary s
    JOIN candidates c ON c.unit_key=pg_catalog.split_part(s.lexical_value,':',7)
    WHERE s.lexical_value LIKE 'urn:tect:dk:unit:'||p_tenant||':'||p_workspace||':%'
      AND ((c.include_revision AND (s.lexical_value=c.ui OR s.lexical_value=c.ri
        OR pg_catalog.left(s.lexical_value,pg_catalog.length(c.ri)+1)=c.ri||':'))
        OR s.lexical_value=c.ep
        OR pg_catalog.left(s.lexical_value,pg_catalog.length(c.ep)+1)=c.ep||':')
    UNION ALL
    SELECT c.input_ordinal,s.id AS subject_id
    FROM pgrdf._pgrdf_dictionary s
    JOIN candidates c ON c.event_key=pg_catalog.split_part(s.lexical_value,':',7)
    WHERE s.lexical_value LIKE 'urn:tect:dk:event:'||p_tenant||':'||p_workspace||':%'
      AND s.lexical_value=c.ei
  ), selected_quads AS MATERIALIZED (
    SELECT ss.input_ordinal,q.subject_id,q.predicate_id,q.object_id
    FROM selected_subjects ss
    JOIN pgrdf._pgrdf_quads q ON q.subject_id=ss.subject_id AND q.graph_id=graph_key
  ), decoded_quads AS MATERIALIZED (
    SELECT q.input_ordinal,c.ui,c.ri,
      s.term_type AS subject_type,s.lexical_value AS subject_value,
      p.term_type AS predicate_type,p.lexical_value AS predicate_value,
      o.term_type AS object_type,o.lexical_value AS object_value,
      o.datatype_iri_id,o.language_tag,dt.lexical_value AS datatype_value,
      (o.datatype_iri_id IS NOT NULL AND
        (dt.id IS NULL OR dt.term_type IS DISTINCT FROM 1 OR dt.lexical_value IS NULL
         OR dt.lexical_value !~ '^[A-Za-z][A-Za-z0-9+.-]*:'
         OR dt.lexical_value ~ '[[:space:]<>"{}|^`\\]')) AS invalid_datatype,
      (s.id IS NULL OR s.term_type IS DISTINCT FROM 1 OR s.lexical_value IS NULL
       OR p.id IS NULL OR p.term_type IS DISTINCT FROM 1 OR p.lexical_value IS NULL
       OR p.lexical_value !~ '^[A-Za-z][A-Za-z0-9+.-]*:'
       OR p.lexical_value ~ '[[:space:]<>"{}|^`\\]'
       OR o.id IS NULL OR o.term_type IS NULL OR o.term_type NOT IN (1,2,3)
       OR o.lexical_value IS NULL
       OR (o.term_type=1 AND
           (o.lexical_value !~ '^[A-Za-z][A-Za-z0-9+.-]*:'
            OR o.lexical_value ~ '[[:space:]<>"{}|^`\\]'))
       OR (s.lexical_value=c.ui
           AND p.lexical_value IN ('http://www.w3.org/1999/02/22-rdf-syntax-ns#type',
                                   'urn:tect:dk:hasRevision')
           AND o.term_type IS DISTINCT FROM 1)) AS malformed_quad
    FROM selected_quads q
    LEFT JOIN pgrdf._pgrdf_dictionary s ON s.id=q.subject_id
    LEFT JOIN pgrdf._pgrdf_dictionary p ON p.id=q.predicate_id
    LEFT JOIN pgrdf._pgrdf_dictionary o ON o.id=q.object_id
    LEFT JOIN pgrdf._pgrdf_dictionary dt ON dt.id=o.datatype_iri_id
    JOIN candidates c ON c.input_ordinal=q.input_ordinal
  )
  SELECT q.input_ordinal,
    pg_catalog.jsonb_build_object(
      'subject',pg_catalog.jsonb_build_object('type',CASE q.subject_type WHEN 1 THEN 'iri' WHEN 2 THEN 'bnode' WHEN 3 THEN 'literal' ELSE 'unsupported' END,'value',q.subject_value),
      'predicate',pg_catalog.jsonb_build_object('type',CASE
        WHEN q.predicate_type=1 AND q.predicate_value ~ '^[A-Za-z][A-Za-z0-9+.-]*:'
          AND q.predicate_value !~ '[[:space:]<>"{}|^`\\]' THEN 'iri'
        ELSE 'unsupported' END,'value',q.predicate_value),
      'object',CASE WHEN q.invalid_datatype THEN
        pg_catalog.jsonb_build_object('type','unsupported','value',q.object_value)
      WHEN q.object_type=1 THEN
        pg_catalog.jsonb_build_object('type','iri','value',q.object_value)
      WHEN q.object_type=2 THEN
        pg_catalog.jsonb_build_object('type','bnode','value',q.object_value)
      WHEN q.object_type=3 THEN
        pg_catalog.jsonb_build_object('type','literal','value',q.object_value,
          'datatype',CASE WHEN q.language_tag IS NOT NULL THEN
            'http://www.w3.org/1999/02/22-rdf-syntax-ns#langString'
          WHEN q.datatype_iri_id IS NULL THEN
            'http://www.w3.org/2001/XMLSchema#string'
          ELSE q.datatype_value END)
        || CASE WHEN q.language_tag IS NOT NULL THEN
          pg_catalog.jsonb_build_object('language',q.language_tag)
        ELSE '{}'::jsonb END
      ELSE pg_catalog.jsonb_build_object('type','unsupported','value',q.object_value)
      END) AS triple
  FROM decoded_quads q
  -- Malformed selected evidence must survive the unit-root selection filter.
  -- Only ordinary, valid unit-root terms can be omitted by the 0012 boundary.
  WHERE q.malformed_quad OR q.invalid_datatype OR q.subject_value<>q.ui OR
        (q.predicate_type=1 AND q.object_type=1 AND
         ((q.predicate_value='http://www.w3.org/1999/02/22-rdf-syntax-ns#type'
           AND q.object_value='urn:tect:dk:KnowledgeUnit') OR
          (q.predicate_value='urn:tect:dk:hasRevision' AND q.object_value=q.ri)));
END
$fn$;

REVOKE ALL PRIVILEGES ON FUNCTION public.tect_dk2_canonical_native_rows(
  uuid,uuid,uuid,uuid[],bigint[],uuid[],boolean[]) FROM PUBLIC;
