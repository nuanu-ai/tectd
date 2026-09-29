-- Exact direct graph assertions for DK-2 native read and erase.
-- Replace only owner-only implementations; migration 0018 public identity guards stay intact.
CREATE OR REPLACE FUNCTION public.tect_dk2_internal_native_read(
  p_tenant uuid,
  p_workspace uuid,
  p_unit uuid,
  p_revision bigint,
  p_event uuid,
  p_include_revision boolean
) RETURNS SETOF jsonb
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $fn$
DECLARE
  stable_iri text;
  unit_iri text;
  revision_iri text;
  event_iri text;
  event_content_prefix text;
  query text;
  assertion_query text;
  assertion_predicates text[] := ARRAY[
    'urn:tect:dk:v2:broaderConcept',
    'urn:tect:dk:v2:classifiedAs',
    'urn:tect:dk:v2:hasEnvironment',
    'urn:tect:dk:v2:appliesTo'
  ];
BEGIN
  IF NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid IS DISTINCT FROM p_tenant
     OR p_revision<1
     OR NOT EXISTS (
       SELECT 1 FROM public.workspaces WHERE tenant_id=p_tenant AND id=p_workspace
     )
     OR NOT EXISTS (
       SELECT 1 FROM public.workspace_knowledge_state
       WHERE tenant_id=p_tenant AND workspace_id=p_workspace AND capability_ready
     ) THEN
    RAISE EXCEPTION USING ERRCODE='42501',MESSAGE='durable knowledge unavailable';
  END IF;
  stable_iri := 'urn:tect:dk:workspace:'||p_tenant||':'||p_workspace;
  unit_iri := 'urn:tect:dk:unit:'||p_tenant||':'||p_workspace||':'||p_unit;
  revision_iri := unit_iri||':revision:'||p_revision;
  event_iri := 'urn:tect:dk:event:'||p_tenant||':'||p_workspace||':'||p_event;
  event_content_prefix := unit_iri||':event:'||p_event;
  query := 'CONSTRUCT { ?s ?p ?o } WHERE { GRAPH <'||stable_iri||'> { '
    ||'?s ?p ?o . FILTER(';
  IF p_include_revision THEN
    query := query
      ||'((?s=<'||unit_iri||'> && '
      ||'((?p=<http://www.w3.org/1999/02/22-rdf-syntax-ns#type> '
      ||'&& ?o=<urn:tect:dk:KnowledgeUnit>) '
      ||'|| (?p=<urn:tect:dk:hasRevision> && ?o=<'||revision_iri||'>))) '
      ||'|| ?s=<'||revision_iri||'> '
      ||'|| STRSTARTS(STR(?s),"'||revision_iri||':")) || ';
  END IF;
  query := query
    ||'?s=<'||event_iri||'> '
    ||'|| ?s=<'||event_content_prefix||'> '
    ||'|| STRSTARTS(STR(?s),"'||event_content_prefix||':")) } }';
  IF NOT p_include_revision THEN
    RETURN QUERY EXECUTE $event_read$
      SELECT native.row
      FROM pgrdf.construct($1) AS native(row)
      WHERE native.row->'predicate'->>'value' <> ALL($2)
    $event_read$ USING query,assertion_predicates;
    RETURN;
  END IF;

  -- Predicate IRIs are fixed by the DK-2 vocabulary. The document's subject
  -- and object IRIs stay SQL values, never interpolated into SPARQL text.
  assertion_query := 'CONSTRUCT { ?s ?p ?o } WHERE { GRAPH <'||stable_iri
    ||'> { ?s ?p ?o . FILTER(?p IN ('
    ||'<urn:tect:dk:v2:broaderConcept>,'
    ||'<urn:tect:dk:v2:classifiedAs>,'
    ||'<urn:tect:dk:v2:hasEnvironment>,'
    ||'<urn:tect:dk:v2:appliesTo>)) } }';
  RETURN QUERY EXECUTE $read$
    WITH stored_assertions AS (
      SELECT assertion->>'subject_iri' AS subject_iri,
             CASE assertion->>'predicate'
               WHEN 'broader_concept' THEN 'urn:tect:dk:v2:broaderConcept'
               WHEN 'classified_as' THEN 'urn:tect:dk:v2:classifiedAs'
               WHEN 'has_environment' THEN 'urn:tect:dk:v2:hasEnvironment'
               WHEN 'applies_to' THEN 'urn:tect:dk:v2:appliesTo'
             END AS predicate_iri,
             assertion->>'object_iri' AS object_iri
      FROM public.knowledge_revisions revision
      CROSS JOIN LATERAL pg_catalog.jsonb_array_elements(
        COALESCE(revision.document_payload->'graph_assertions','[]'::jsonb)
      ) AS assertion
      WHERE revision.tenant_id=$3 AND revision.workspace_id=$4
        AND revision.unit_id=$5 AND revision.revision=$6
        AND NOT revision.payload_erased
    ),
    owned AS (
      SELECT native.row
      FROM pgrdf.construct($1) AS native(row)
      WHERE native.row->'predicate'->>'value' <> ALL($7)
    ),
    asserted AS (
      SELECT native.row
      FROM pgrdf.construct($2) AS native(row)
      JOIN stored_assertions assertion
        ON native.row->'subject'->>'value'=assertion.subject_iri
       AND native.row->'predicate'->>'value'=assertion.predicate_iri
       AND native.row->'object'->>'value'=assertion.object_iri
       AND native.row->'subject'->>'type'='iri'
       AND native.row->'predicate'->>'type'='iri'
       AND native.row->'object'->>'type'='iri'
    )
    SELECT DISTINCT row FROM (
      SELECT row FROM owned UNION ALL SELECT row FROM asserted
    ) AS selected
  $read$
  USING query,assertion_query,p_tenant,p_workspace,p_unit,p_revision,
        assertion_predicates;
END
$fn$;

REVOKE ALL PRIVILEGES ON FUNCTION
  public.tect_dk2_internal_native_read(uuid,uuid,uuid,bigint,uuid,boolean)
FROM PUBLIC;

CREATE OR REPLACE FUNCTION public.tect_dk_internal_native_erase(p_tenant uuid,p_workspace uuid,p_unit uuid)
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
  unshared_assertions jsonb := '[]'::jsonb;
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

  -- Every historical revision of this unit is erased together. A direct
  -- triple remains in the workspace graph while any other unit's non-erased
  -- historical revision still asserts the same complete triple.
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
    WHERE (
      (
        line LIKE ('<' || $2 || '> %')
        OR line LIKE ('<' || $2 || ':%')
        OR EXISTS (
          SELECT 1 FROM pg_catalog.unnest($3) AS owned_event(iri)
          WHERE line LIKE ('<' || owned_event.iri || '> %')
        )
      )
      AND pg_catalog.split_part(line,' ',2) NOT IN (
        '<urn:tect:dk:v2:broaderConcept>',
        '<urn:tect:dk:v2:classifiedAs>',
        '<urn:tect:dk:v2:hasEnvironment>',
        '<urn:tect:dk:v2:appliesTo>'
      )
    )
       OR EXISTS (
         SELECT 1
         FROM pg_catalog.jsonb_array_elements($4) AS assertion
         WHERE line = '<'||(assertion->>'subject_iri')||'> <'
           ||(assertion->>'predicate_iri')||'> <'
           ||(assertion->>'object_iri')||'> .'
       )
  $export$
  INTO target_lines,target_bytes,target_rows
  USING stable_graph_id,unit_iri,publication_event_iris,unshared_assertions;

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
      JOIN pgrdf._pgrdf_dictionary predicate ON predicate.id = q.predicate_id
      JOIN pgrdf._pgrdf_dictionary object_term ON object_term.id = q.object_id
      WHERE q.graph_id = $1
        AND (
          (
            subject.lexical_value = $2
            OR subject.lexical_value LIKE ($2 || ':%')
            OR subject.lexical_value = ANY($3)
          )
          AND predicate.lexical_value NOT IN (
            'urn:tect:dk:v2:broaderConcept',
            'urn:tect:dk:v2:classifiedAs',
            'urn:tect:dk:v2:hasEnvironment',
            'urn:tect:dk:v2:appliesTo'
          )
          OR EXISTS (
            SELECT 1
            FROM pg_catalog.jsonb_array_elements($4) AS assertion
            WHERE subject.term_type=1 AND predicate.term_type=1
              AND object_term.term_type=1
              AND subject.lexical_value=assertion->>'subject_iri'
              AND predicate.lexical_value=assertion->>'predicate_iri'
              AND object_term.lexical_value=assertion->>'object_iri'
          )
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
  USING stable_graph_id,unit_iri,publication_event_iris,unshared_assertions;

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
    JOIN pgrdf._pgrdf_dictionary predicate ON predicate.id = q.predicate_id
    JOIN pgrdf._pgrdf_dictionary object_term ON object_term.id = q.object_id
    WHERE q.graph_id = $1
      AND (
        (
          subject.lexical_value = $2
          OR subject.lexical_value LIKE ($2 || ':%')
          OR subject.lexical_value = ANY($3)
        )
        AND predicate.lexical_value NOT IN (
          'urn:tect:dk:v2:broaderConcept',
          'urn:tect:dk:v2:classifiedAs',
          'urn:tect:dk:v2:hasEnvironment',
          'urn:tect:dk:v2:appliesTo'
        )
        OR EXISTS (
          SELECT 1
          FROM pg_catalog.jsonb_array_elements($4) AS assertion
          WHERE subject.term_type=1 AND predicate.term_type=1
            AND object_term.term_type=1
            AND subject.lexical_value=assertion->>'subject_iri'
            AND predicate.lexical_value=assertion->>'predicate_iri'
            AND object_term.lexical_value=assertion->>'object_iri'
        )
      )
  $remaining$
  INTO remaining_owned_rows
  USING stable_graph_id,unit_iri,publication_event_iris,unshared_assertions;

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

REVOKE ALL PRIVILEGES ON FUNCTION public.tect_dk_internal_native_erase(uuid,uuid,uuid) FROM PUBLIC;
