-- Read the already authorized search corpus's RDF rows in one workspace graph
-- pass. The caller still verifies each publication, native triples and receipt.
CREATE FUNCTION public.tect_dk2_search_native_rows(
  p_tenant uuid, p_workspace uuid, p_principal uuid,
  p_units uuid[], p_revisions bigint[], p_events uuid[]
) RETURNS TABLE(unit_id uuid, triple jsonb)
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $fn$
DECLARE
  caller_is_admin boolean;
  graph_iri text;
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
  graph_iri := 'urn:tect:dk:workspace:'||p_tenant||':'||p_workspace;
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
  ), triples AS MATERIALIZED (
    SELECT x AS triple FROM pgrdf.construct(
      'CONSTRUCT { ?s ?p ?o } WHERE { GRAPH <'||graph_iri||'> { ?s ?p ?o } }') x
  ), normalized AS MATERIALIZED (
    SELECT t.triple,t.triple->'subject'->>'value' AS subject,
      t.triple->'predicate'->>'value' AS predicate,t.triple->'object'->>'value' AS object,
      pg_catalog.split_part(t.triple->'subject'->>'value',':',4) AS subject_kind,
      pg_catalog.split_part(t.triple->'subject'->>'value',':',7) AS subject_key
    FROM triples t
  )
  SELECT c.unit_id,n.triple FROM normalized n JOIN candidates c ON c.unit_key=n.subject_key
  WHERE n.subject_kind='unit' AND (
    (n.subject=c.ui AND ((n.predicate='http://www.w3.org/1999/02/22-rdf-syntax-ns#type'
      AND n.object='urn:tect:dk:KnowledgeUnit') OR
      (n.predicate='urn:tect:dk:hasRevision' AND n.object=c.ri)))
    OR n.subject=c.ri OR pg_catalog.left(n.subject,pg_catalog.length(c.ri)+1)=c.ri||':'
    OR n.subject=c.ep OR pg_catalog.left(n.subject,pg_catalog.length(c.ep)+1)=c.ep||':'
  )
  UNION ALL
  SELECT c.unit_id,n.triple FROM normalized n JOIN candidates c ON c.event_key=n.subject_key
  WHERE n.subject_kind='event' AND n.subject=c.ei;
END
$fn$;

REVOKE ALL PRIVILEGES ON FUNCTION public.tect_dk2_search_native_rows(
  uuid,uuid,uuid,uuid[],bigint[],uuid[]) FROM PUBLIC;
