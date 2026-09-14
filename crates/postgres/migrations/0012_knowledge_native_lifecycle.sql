-- DK-2 strict typed RDF adapter. pgRDF references stay dynamic so metadata
-- migration succeeds before operator-only extension activation.
CREATE FUNCTION public.tect_dk2_native_publish(
  p_tenant uuid,
  p_workspace uuid,
  p_event uuid,
  p_operation text,
  p_payload text,
  p_stable_payload text
) RETURNS text
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $fn$
DECLARE
  scratch_iri text;
  stable_iri text;
  scratch_id bigint;
  stable_id bigint;
  shapes_id bigint;
  report jsonb;
  result text;
  typed_targets bigint;
BEGIN
  IF NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid IS DISTINCT FROM p_tenant
     OR NOT EXISTS (
       SELECT 1 FROM public.workspaces WHERE tenant_id=p_tenant AND id=p_workspace
     )
     OR NOT EXISTS (
       SELECT 1 FROM public.workspace_knowledge_state
       WHERE tenant_id=p_tenant AND workspace_id=p_workspace AND capability_ready
     ) THEN
    RAISE EXCEPTION USING ERRCODE='42501',MESSAGE='durable knowledge unavailable';
  END IF;
  IF p_operation NOT IN ('create','revise','revalidate','supersede','retract')
     OR pg_catalog.octet_length(p_payload)=0
     OR pg_catalog.octet_length(p_stable_payload)=0
     OR pg_catalog.octet_length(p_payload)>8388608
     OR pg_catalog.octet_length(p_stable_payload)>8388608 THEN
    RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid DK-2 native publication';
  END IF;
  PERFORM pg_catalog.pg_advisory_xact_lock(
    pg_catalog.hashtextextended('tect-dk-native-publisher',0)
  );
  scratch_iri := 'urn:tect:dk:scratch:dk-2:'||p_tenant||':'||p_workspace||':'||p_event;
  stable_iri := 'urn:tect:dk:workspace:'||p_tenant||':'||p_workspace;
  EXECUTE 'SELECT pgrdf.add_graph($1)' INTO scratch_id USING scratch_iri;
  EXECUTE 'SELECT pgrdf.clear_graph($1)' USING scratch_id;
  EXECUTE 'SELECT pgrdf.parse_turtle($1,$2)' USING p_payload,scratch_id;
  EXECUTE 'SELECT pgrdf.graph_id($1)' INTO shapes_id USING 'urn:tect:dk:shapes:dk-2';
  IF shapes_id IS NULL THEN
    RAISE EXCEPTION USING ERRCODE='55000',MESSAGE='DK-2 native shapes unavailable';
  END IF;
  EXECUTE 'SELECT pgrdf.validate($1,$2,''native'',true)'
    INTO report USING scratch_id,shapes_id;
  EXECUTE $typed$
    SELECT count(*) FROM pgrdf.construct($1)
  $typed$ INTO typed_targets USING
    'CONSTRUCT { ?s ?p ?o } WHERE { GRAPH <'||scratch_iri||'> { '
    ||'?s <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> ?type ; ?p ?o . '
    ||'FILTER(?type IN (<urn:tect:dk:v2:KnowledgeRevision>,<urn:tect:dk:v2:PublicationEvent>)) } }';
  IF report->>'conforms' IS DISTINCT FROM 'true' OR typed_targets=0 THEN
    RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='DK-2 native shape rejected';
  END IF;
  EXECUTE 'SELECT pgrdf.graph_digest($1)' INTO result USING scratch_id;
  EXECUTE 'SELECT pgrdf.add_graph($1)' INTO stable_id USING stable_iri;
  EXECUTE 'SELECT pgrdf.parse_turtle($1,$2)' USING p_stable_payload,stable_id;
  EXECUTE 'SELECT pgrdf.drop_graph($1,true)' USING scratch_id;
  RETURN result;
END
$fn$;

CREATE FUNCTION public.tect_dk2_native_read(
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
  RETURN QUERY EXECUTE 'SELECT * FROM pgrdf.construct($1)' USING query;
END
$fn$;

REVOKE ALL PRIVILEGES ON FUNCTION
  public.tect_dk2_native_publish(uuid,uuid,uuid,text,text,text),
  public.tect_dk2_native_read(uuid,uuid,uuid,bigint,uuid,boolean)
FROM PUBLIC;
