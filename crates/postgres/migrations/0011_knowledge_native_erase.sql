-- DK-2 logical live-row erase adapter for pgRDF 0.6.34.
-- The 8 MiB limit is the UTF-8 byte length of the canonical, newline-joined
-- N-Triples selected from pgrdf.export_graph for one unit transaction.
-- This function is coupled to the validated pgRDF 0.6.34 private table shape;
-- it does not claim forensic removal from WAL, MVCC history, dumps, or backups.
CREATE FUNCTION public.tect_dk_native_erase(p_tenant uuid,p_workspace uuid,p_unit uuid)
RETURNS jsonb
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $fn$
DECLARE
  caller_is_admin boolean;
  stable_graph text;
  unit_iri text;
  publication_event_iris text[];
  stable_graph_id bigint;
  native_version text;
  native_build_id text;
  extension_version text;
  schema_compatible boolean;
  target_lines text;
  target_bytes bigint := 0;
  target_rows bigint := 0;
  deleted_rows bigint := 0;
  remaining_owned_rows bigint := 0;
  literal_or_blanknode_terms_deleted bigint := 0;
  uri_terms_deleted bigint := 0;
  captured_ids bigint[] := ARRAY[]::bigint[];
  delete_update text;
BEGIN
  SELECT pg_catalog.pg_has_role(SESSION_USER,d.datdba,'MEMBER')
  INTO caller_is_admin
  FROM pg_catalog.pg_database d
  WHERE d.datname=pg_catalog.current_database();

  IF (caller_is_admin IS DISTINCT FROM true AND
        NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid IS DISTINCT FROM p_tenant)
     OR NOT EXISTS (
       SELECT 1 FROM public.workspaces
       WHERE tenant_id = p_tenant AND id = p_workspace
     )
     OR NOT EXISTS (
       SELECT 1 FROM public.workspace_knowledge_state
       WHERE tenant_id = p_tenant AND workspace_id = p_workspace AND capability_ready
     ) THEN
    RAISE EXCEPTION USING ERRCODE = '42501', MESSAGE = 'durable knowledge unavailable';
  END IF;

  IF NOT EXISTS (
    SELECT 1 FROM public.knowledge_unit_heads
    WHERE tenant_id = p_tenant AND workspace_id = p_workspace AND unit_id = p_unit
  ) AND caller_is_admin IS DISTINCT FROM true THEN
    RAISE EXCEPTION USING ERRCODE = '22023', MESSAGE = 'durable knowledge unit not found';
  END IF;

  -- Serialize every native publisher and erase operation through one global gate.
  PERFORM pg_catalog.pg_advisory_xact_lock(
    pg_catalog.hashtextextended('tect-dk-native-publisher', 0)
  );

  -- The adapter intentionally fails closed before any native mutation if the
  -- installed engine or the private table shape differs from the pinned build.
  IF pg_catalog.to_regnamespace('pgrdf') IS NULL THEN
    RAISE EXCEPTION USING ERRCODE = '55000', MESSAGE = 'durable knowledge native erase unavailable';
  END IF;

  BEGIN
    EXECUTE 'SELECT pgrdf.version(),pgrdf.build_id()' INTO native_version,native_build_id;
    SELECT extversion INTO extension_version
    FROM pg_catalog.pg_extension WHERE extname = 'pgrdf';

    EXECUTE $shape$
      WITH expected(relation_name,column_name,type_name) AS (
        VALUES
          ('_pgrdf_dictionary','id','int8'),
          ('_pgrdf_dictionary','term_type','int2'),
          ('_pgrdf_dictionary','lexical_value','text'),
          ('_pgrdf_dictionary','datatype_iri_id','int8'),
          ('_pgrdf_quads','subject_id','int8'),
          ('_pgrdf_quads','predicate_id','int8'),
          ('_pgrdf_quads','object_id','int8'),
          ('_pgrdf_quads','graph_id','int8'),
          ('_pgrdf_graphs','graph_id','int8'),
          ('_pgrdf_graphs','iri','text')
      )
      SELECT count(*) = 10
      FROM expected e
      JOIN pg_catalog.pg_namespace n ON n.nspname = 'pgrdf'
      JOIN pg_catalog.pg_class c ON c.relnamespace = n.oid AND c.relname = e.relation_name
      JOIN pg_catalog.pg_attribute a ON a.attrelid = c.oid
                                    AND a.attname = e.column_name
                                    AND NOT a.attisdropped
      JOIN pg_catalog.pg_type t ON t.oid = a.atttypid AND t.typname = e.type_name
    $shape$ INTO schema_compatible;
  EXCEPTION WHEN undefined_function OR undefined_table OR undefined_column THEN
    RAISE EXCEPTION USING ERRCODE = '55000', MESSAGE = 'durable knowledge native erase unavailable';
  END;

  IF native_version IS DISTINCT FROM '0.6.34'
     OR native_build_id IS DISTINCT FROM 'v0.6.34'
     OR extension_version IS DISTINCT FROM '0.6.34'
     OR schema_compatible IS DISTINCT FROM true
     OR pg_catalog.to_regprocedure('pgrdf.export_graph(bigint)') IS NULL
     OR pg_catalog.to_regprocedure('pgrdf.graph_id(text)') IS NULL
     OR pg_catalog.to_regprocedure('pgrdf.shmem_reset()') IS NULL
     OR pg_catalog.to_regprocedure('pgrdf.sparql(text)') IS NULL THEN
    RAISE EXCEPTION USING ERRCODE = '55000', MESSAGE = 'unsupported durable knowledge native erase engine';
  END IF;

  stable_graph := 'urn:tect:dk:workspace:' || p_tenant || ':' || p_workspace;
  unit_iri := 'urn:tect:dk:unit:' || p_tenant || ':' || p_workspace || ':' || p_unit;
  SELECT COALESCE(
           pg_catalog.array_agg(
             'urn:tect:dk:event:' || p_tenant || ':' || p_workspace || ':' || id
             ORDER BY id
           ),
           ARRAY[]::text[]
         )
  INTO publication_event_iris
  FROM public.knowledge_publication_events
  WHERE tenant_id = p_tenant AND workspace_id = p_workspace AND unit_id = p_unit;

  EXECUTE 'SELECT pgrdf.graph_id($1)' INTO stable_graph_id USING stable_graph;
  IF stable_graph_id IS NULL THEN
    RAISE EXCEPTION USING ERRCODE = '55000', MESSAGE = 'durable knowledge stable graph unavailable';
  END IF;

  -- Keep the exact canonical export lines. Ownership is constrained only by
  -- the server-built unit subject identity and event IDs already stored for
  -- this tenant/workspace/unit; callers cannot supply graph IDs or subjects.
  EXECUTE $export$
    SELECT
      COALESCE(
        pg_catalog.string_agg(line, E'\n' ORDER BY line COLLATE "C"),
        ''
      ),
      COALESCE(pg_catalog.sum(pg_catalog.octet_length(line)), 0)
        + GREATEST(pg_catalog.count(*) - 1, 0),
      pg_catalog.count(*)
    FROM pgrdf.export_graph($1) AS exported(line)
    WHERE line LIKE ('<' || $2 || '> %')
       OR line LIKE ('<' || $2 || ':%')
       OR EXISTS (
         SELECT 1 FROM pg_catalog.unnest($3) AS owned_event(iri)
         WHERE line LIKE ('<' || owned_event.iri || '> %')
       )
  $export$
  INTO target_lines,target_bytes,target_rows
  USING stable_graph_id,unit_iri,publication_event_iris;

  IF target_bytes > 8388608 THEN
    RAISE EXCEPTION USING ERRCODE = '54000', MESSAGE = 'durable knowledge native erase exceeds 8 MiB unit limit';
  END IF;

  -- Capture only IDs reachable from the owned target triples, plus the direct
  -- datatype IDs of those terms, before DELETE DATA changes the quad rows.
  EXECUTE $capture$
    WITH owned_quads AS (
      SELECT q.subject_id,q.predicate_id,q.object_id
      FROM pgrdf._pgrdf_quads q
      JOIN pgrdf._pgrdf_dictionary subject ON subject.id = q.subject_id
      WHERE q.graph_id = $1
        AND (
          subject.lexical_value = $2
          OR subject.lexical_value LIKE ($2 || ':%')
          OR subject.lexical_value = ANY($3)
        )
    ),
    base_ids AS (
      SELECT subject_id AS id FROM owned_quads
      UNION SELECT predicate_id FROM owned_quads
      UNION SELECT object_id FROM owned_quads
    ),
    target_ids AS (
      SELECT id FROM base_ids
      UNION
      SELECT dictionary.datatype_iri_id
      FROM pgrdf._pgrdf_dictionary dictionary
      JOIN base_ids ON base_ids.id = dictionary.id
      WHERE dictionary.datatype_iri_id IS NOT NULL
    )
    SELECT COALESCE(pg_catalog.array_agg(id ORDER BY id), ARRAY[]::bigint[])
    FROM target_ids
  $capture$
  INTO captured_ids
  USING stable_graph_id,unit_iri,publication_event_iris;

  IF target_rows > 0 THEN
    delete_update := 'DELETE DATA { GRAPH <' || stable_graph || '> {'
      || E'\n' || target_lines || E'\n' || '} }';
    EXECUTE $delete$
      SELECT COALESCE(
        pg_catalog.sum((result->'_update'->>'triples_deleted')::bigint),
        0
      )
      FROM pgrdf.sparql($1) AS update_result(result)
    $delete$
    INTO deleted_rows
    USING delete_update;
  END IF;

  EXECUTE $remaining$
    SELECT pg_catalog.count(*)
    FROM pgrdf._pgrdf_quads q
    JOIN pgrdf._pgrdf_dictionary subject ON subject.id = q.subject_id
    WHERE q.graph_id = $1
      AND (
        subject.lexical_value = $2
        OR subject.lexical_value LIKE ($2 || ':%')
        OR subject.lexical_value = ANY($3)
      )
  $remaining$
  INTO remaining_owned_rows
  USING stable_graph_id,unit_iri,publication_event_iris;

  IF deleted_rows <> target_rows OR remaining_owned_rows <> 0 THEN
    RAISE EXCEPTION USING ERRCODE = '55000', MESSAGE = 'durable knowledge native erase incomplete';
  END IF;

  EXECUTE $purge_literals$
    WITH removed AS (
      DELETE FROM pgrdf._pgrdf_dictionary dictionary
      WHERE dictionary.id = ANY($1)
        AND dictionary.term_type IN (2,3)
        AND NOT EXISTS (
          SELECT 1 FROM pgrdf._pgrdf_quads q
          WHERE q.subject_id = dictionary.id
             OR q.predicate_id = dictionary.id
             OR q.object_id = dictionary.id
        )
        AND NOT EXISTS (
          SELECT 1 FROM pgrdf._pgrdf_dictionary dependent
          WHERE dependent.datatype_iri_id = dictionary.id
        )
        AND NOT EXISTS (
          SELECT 1 FROM pgrdf._pgrdf_graphs graph
          WHERE graph.iri = dictionary.lexical_value
        )
      RETURNING 1
    )
    SELECT pg_catalog.count(*) FROM removed
  $purge_literals$
  INTO literal_or_blanknode_terms_deleted
  USING captured_ids;

  EXECUTE $purge_uris$
    WITH removed AS (
      DELETE FROM pgrdf._pgrdf_dictionary dictionary
      WHERE dictionary.id = ANY($1)
        AND dictionary.term_type = 1
        AND NOT EXISTS (
          SELECT 1 FROM pgrdf._pgrdf_quads q
          WHERE q.subject_id = dictionary.id
             OR q.predicate_id = dictionary.id
             OR q.object_id = dictionary.id
        )
        AND NOT EXISTS (
          SELECT 1 FROM pgrdf._pgrdf_dictionary dependent
          WHERE dependent.datatype_iri_id = dictionary.id
        )
        AND NOT EXISTS (
          SELECT 1 FROM pgrdf._pgrdf_graphs graph
          WHERE graph.iri = dictionary.lexical_value
        )
      RETURNING 1
    )
    SELECT pg_catalog.count(*) FROM removed
  $purge_uris$
  INTO uri_terms_deleted
  USING captured_ids;

  EXECUTE 'SELECT pgrdf.shmem_reset()';

  RETURN pg_catalog.jsonb_build_object(
    'target_bytes',target_bytes,
    'triples_deleted',deleted_rows,
    'dictionary_literal_or_blanknode_terms_deleted',literal_or_blanknode_terms_deleted,
    'dictionary_uri_terms_deleted',uri_terms_deleted,
    'dictionary_terms_deleted',literal_or_blanknode_terms_deleted + uri_terms_deleted
  );
END
$fn$;

REVOKE ALL PRIVILEGES ON FUNCTION public.tect_dk_native_erase(uuid,uuid,uuid) FROM PUBLIC;

-- Count only server-derived subjects owned by one unit. This wrapper is the
-- runtime-safe residual surface; callers never receive native graph access.
CREATE FUNCTION public.tect_dk_native_owned_residual(p_tenant uuid,p_workspace uuid,p_unit uuid)
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
          ('_pgrdf_dictionary','lexical_value','text'),
          ('_pgrdf_quads','subject_id','int8'),
          ('_pgrdf_quads','predicate_id','int8'),
          ('_pgrdf_quads','object_id','int8'),
          ('_pgrdf_quads','graph_id','int8'),
          ('_pgrdf_graphs','graph_id','int8'),
          ('_pgrdf_graphs','iri','text')
      )
      SELECT count(*)=8
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
  RETURN pg_catalog.jsonb_build_object('owned_triples',owned_triples);
END
$fn$;

REVOKE ALL PRIVILEGES ON FUNCTION public.tect_dk_native_owned_residual(uuid,uuid,uuid) FROM PUBLIC;
