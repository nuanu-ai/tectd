-- Count only server-derived subjects owned by one unit. This wrapper is the
-- runtime-safe residual surface; callers never receive native graph access.
CREATE OR REPLACE FUNCTION public.tect_dk_internal_native_owned_residual(p_tenant uuid,p_workspace uuid,p_unit uuid)
RETURNS jsonb
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $fn$
DECLARE
  caller_is_admin boolean;
  workspace_exists boolean;
  stable_graph text;
  stable_graph_id bigint;
  unit_iri text;
  publication_event_iris text[];
  native_version text;
  native_build_id text;
  extension_version text;
  schema_compatible boolean;
  owned_triples bigint := 0;
  direct_triples bigint := 0;
  excluded_direct_predicate_triples bigint := 0;
  unshared_assertions jsonb := '[]'::jsonb;
  query text;
BEGIN
  SELECT pg_catalog.pg_has_role(SESSION_USER,d.datdba,'MEMBER')
  INTO caller_is_admin
  FROM pg_catalog.pg_database d WHERE d.datname=pg_catalog.current_database();
  SELECT EXISTS(SELECT 1 FROM public.workspaces WHERE tenant_id=p_tenant AND id=p_workspace)
  INTO workspace_exists;
  IF (NOT caller_is_admin AND (
        NOT workspace_exists
        OR NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid IS DISTINCT FROM p_tenant
        OR NOT EXISTS (
          SELECT 1 FROM public.workspace_knowledge_state
          WHERE tenant_id=p_tenant AND workspace_id=p_workspace AND capability_ready
        )
      ))
     OR (NOT workspace_exists AND NOT caller_is_admin) THEN
    RAISE EXCEPTION USING ERRCODE='42501',MESSAGE='durable knowledge unavailable';
  END IF;
  PERFORM pg_catalog.pg_advisory_xact_lock(
    pg_catalog.hashtextextended('tect-dk-native-publisher',0)
  );
  IF pg_catalog.to_regnamespace('pgrdf') IS NULL THEN
    RAISE EXCEPTION USING ERRCODE='55000',MESSAGE='durable knowledge native residual unavailable';
  END IF;
  BEGIN
    EXECUTE 'SELECT pgrdf.version(),pgrdf.build_id()' INTO native_version,native_build_id;
    SELECT extversion INTO extension_version FROM pg_catalog.pg_extension WHERE extname='pgrdf';
    EXECUTE $shape$
      WITH expected(relation_name,column_name,type_name) AS (
        VALUES
          ('_pgrdf_dictionary','id','int8'),
          ('_pgrdf_dictionary','term_type','int2'),
          ('_pgrdf_dictionary','lexical_value','text'),
          ('_pgrdf_quads','subject_id','int8'),
          ('_pgrdf_quads','predicate_id','int8'),
          ('_pgrdf_quads','object_id','int8'),
          ('_pgrdf_quads','graph_id','int8'),
          ('_pgrdf_graphs','graph_id','int8'),
          ('_pgrdf_graphs','iri','text')
      )
      SELECT count(*)=9
      FROM expected e
      JOIN pg_catalog.pg_namespace n ON n.nspname='pgrdf'
      JOIN pg_catalog.pg_class c ON c.relnamespace=n.oid AND c.relname=e.relation_name
      JOIN pg_catalog.pg_attribute a ON a.attrelid=c.oid AND a.attname=e.column_name AND NOT a.attisdropped
      JOIN pg_catalog.pg_type t ON t.oid=a.atttypid AND t.typname=e.type_name
    $shape$ INTO schema_compatible;
  EXCEPTION WHEN undefined_function OR undefined_table OR undefined_column THEN
    RAISE EXCEPTION USING ERRCODE='55000',MESSAGE='durable knowledge native residual unavailable';
  END;
  IF native_version IS DISTINCT FROM '0.6.34'
     OR native_build_id IS DISTINCT FROM 'v0.6.34'
     OR extension_version IS DISTINCT FROM '0.6.34'
     OR schema_compatible IS DISTINCT FROM true
     OR pg_catalog.to_regprocedure('pgrdf.graph_id(text)') IS NULL
     OR pg_catalog.to_regprocedure('pgrdf.construct(text)') IS NULL THEN
    RAISE EXCEPTION USING ERRCODE='55000',MESSAGE='unsupported durable knowledge native residual engine';
  END IF;
  stable_graph := 'urn:tect:dk:workspace:'||p_tenant||':'||p_workspace;
  unit_iri := 'urn:tect:dk:unit:'||p_tenant||':'||p_workspace||':'||p_unit;
  SELECT COALESCE(
    pg_catalog.array_agg('urn:tect:dk:event:'||p_tenant||':'||p_workspace||':'||id ORDER BY id),
    ARRAY[]::text[]
  ) INTO publication_event_iris
  FROM public.knowledge_publication_events
  WHERE tenant_id=p_tenant AND workspace_id=p_workspace AND unit_id=p_unit;
  EXECUTE 'SELECT pgrdf.graph_id($1)' INTO stable_graph_id USING stable_graph;
  IF stable_graph_id IS NULL THEN
    RETURN pg_catalog.jsonb_build_object('owned_triples',0);
  END IF;
  query := 'CONSTRUCT { ?s ?p ?o } WHERE { GRAPH <'||stable_graph||'> { ?s ?p ?o . FILTER('
    ||'?s=<'||unit_iri||'> || STRSTARTS(STR(?s),"'||unit_iri||':")';
  IF pg_catalog.cardinality(publication_event_iris)>0 THEN
    query := query||' || ?s IN ('||(
      SELECT pg_catalog.string_agg('<'||iri||'>',',' ORDER BY iri COLLATE "C")
      FROM pg_catalog.unnest(publication_event_iris) AS event(iri)
    )||')';
  END IF;
  query := query||') } }';
  EXECUTE 'SELECT count(*) FROM pgrdf.construct($1)' INTO owned_triples USING query;

  -- The generated-subject scan owns structural predicates only. Direct
  -- predicates are counted below by exact stored assertion tuple.
  EXECUTE $excluded_direct$
    SELECT pg_catalog.count(*)
    FROM pgrdf._pgrdf_quads quad
    JOIN pgrdf._pgrdf_dictionary subject ON subject.id=quad.subject_id
    JOIN pgrdf._pgrdf_dictionary predicate ON predicate.id=quad.predicate_id
    WHERE quad.graph_id=$1
      AND (
        subject.lexical_value=$2
        OR subject.lexical_value LIKE ($2||':%')
        OR subject.lexical_value=ANY($3)
      )
      AND predicate.lexical_value IN (
        'urn:tect:dk:v2:broaderConcept',
        'urn:tect:dk:v2:classifiedAs',
        'urn:tect:dk:v2:hasEnvironment',
        'urn:tect:dk:v2:appliesTo'
      )
  $excluded_direct$
  INTO excluded_direct_predicate_triples
  USING stable_graph_id,unit_iri,publication_event_iris;

  SELECT COALESCE(pg_catalog.jsonb_agg(DISTINCT pg_catalog.jsonb_build_object(
           'subject_iri', assertion->>'subject_iri',
           'predicate_iri', CASE assertion->>'predicate'
             WHEN 'broader_concept' THEN 'urn:tect:dk:v2:broaderConcept'
             WHEN 'classified_as' THEN 'urn:tect:dk:v2:classifiedAs'
             WHEN 'has_environment' THEN 'urn:tect:dk:v2:hasEnvironment'
             WHEN 'applies_to' THEN 'urn:tect:dk:v2:appliesTo'
           END,
           'object_iri', assertion->>'object_iri'
         )), '[]'::jsonb)
  INTO unshared_assertions
  FROM public.knowledge_revisions revision
  CROSS JOIN LATERAL pg_catalog.jsonb_array_elements(
    COALESCE(revision.document_payload->'graph_assertions','[]'::jsonb)
  ) AS assertion
  WHERE revision.tenant_id=p_tenant AND revision.workspace_id=p_workspace
    AND revision.unit_id=p_unit AND NOT revision.payload_erased
    AND NOT EXISTS (
      SELECT 1
      FROM public.knowledge_revisions other_revision
      CROSS JOIN LATERAL pg_catalog.jsonb_array_elements(
        COALESCE(other_revision.document_payload->'graph_assertions','[]'::jsonb)
      ) AS other_assertion
      WHERE other_revision.tenant_id=p_tenant
        AND other_revision.workspace_id=p_workspace
        AND other_revision.unit_id<>p_unit
        AND NOT other_revision.payload_erased
        AND other_assertion->>'subject_iri'=assertion->>'subject_iri'
        AND other_assertion->>'predicate'=assertion->>'predicate'
        AND other_assertion->>'object_iri'=assertion->>'object_iri'
    );
  EXECUTE $direct$
    SELECT pg_catalog.count(*)
    FROM pgrdf._pgrdf_quads quad
    JOIN pgrdf._pgrdf_dictionary subject ON subject.id=quad.subject_id
    JOIN pgrdf._pgrdf_dictionary predicate ON predicate.id=quad.predicate_id
    JOIN pgrdf._pgrdf_dictionary object_term ON object_term.id=quad.object_id
    WHERE quad.graph_id=$1
      AND EXISTS (
        SELECT 1
        FROM pg_catalog.jsonb_array_elements($2) AS assertion
        WHERE subject.term_type=1 AND predicate.term_type=1
          AND object_term.term_type=1
          AND subject.lexical_value=assertion->>'subject_iri'
          AND predicate.lexical_value=assertion->>'predicate_iri'
          AND object_term.lexical_value=assertion->>'object_iri'
      )
  $direct$
  INTO direct_triples
  USING stable_graph_id,unshared_assertions;
  RETURN pg_catalog.jsonb_build_object(
    'owned_triples',owned_triples-excluded_direct_predicate_triples+direct_triples
  );
END
$fn$;

REVOKE ALL PRIVILEGES ON FUNCTION public.tect_dk_internal_native_owned_residual(uuid,uuid,uuid) FROM PUBLIC;
