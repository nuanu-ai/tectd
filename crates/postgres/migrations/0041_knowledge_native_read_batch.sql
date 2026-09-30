-- Candidate batch adapter. Scalar readers and migration 0040 semantics stay intact.
CREATE FUNCTION public.tect_dk2_internal_native_read_batch(
  p_tenant uuid, p_workspace uuid, p_requests jsonb
) RETURNS TABLE(request_ordinal bigint,unit_id uuid,revision bigint,event_id uuid,
                include_revision boolean,triple jsonb)
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public
AS $fn$
DECLARE
  native_version text;
  native_build text;
  extension_version text;
  compatible boolean;
  selected record;
BEGIN
  IF NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid IS DISTINCT FROM p_tenant
     OR NOT EXISTS(SELECT 1 FROM public.workspaces w WHERE w.tenant_id=p_tenant AND w.id=p_workspace)
     OR NOT EXISTS(SELECT 1 FROM public.workspace_knowledge_state k
                   WHERE k.tenant_id=p_tenant AND k.workspace_id=p_workspace AND k.capability_ready) THEN
    RAISE EXCEPTION USING ERRCODE='42501',MESSAGE='durable knowledge unavailable';
  END IF;
  IF p_requests IS NULL OR pg_catalog.jsonb_typeof(p_requests)<>'array' THEN
    RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid native batch requests';
  END IF;
  FOR selected IN SELECT value FROM pg_catalog.jsonb_array_elements(p_requests) LOOP
    IF pg_catalog.jsonb_typeof(selected.value)<>'object' THEN
      RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid native batch request';
    END IF;
    IF (SELECT count(*) FROM pg_catalog.jsonb_object_keys(selected.value))<>4
       OR NOT selected.value ?& ARRAY['unit_id','revision','event_id','include_revision']
       OR pg_catalog.jsonb_typeof(selected.value->'unit_id') IS DISTINCT FROM 'string'
       OR pg_catalog.jsonb_typeof(selected.value->'event_id') IS DISTINCT FROM 'string'
       OR pg_catalog.jsonb_typeof(selected.value->'revision') IS DISTINCT FROM 'number'
       OR (selected.value->>'revision') !~ '^[0-9]+$'
       OR pg_catalog.jsonb_typeof(selected.value->'include_revision') IS DISTINCT FROM 'boolean' THEN
      RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid native batch request';
    END IF;
    BEGIN
      PERFORM (selected.value->>'unit_id')::uuid,(selected.value->>'event_id')::uuid;
      IF (selected.value->>'revision')::bigint<1 THEN
        RAISE EXCEPTION USING ERRCODE='42501',MESSAGE='durable knowledge unavailable';
      END IF;
    EXCEPTION WHEN invalid_text_representation OR numeric_value_out_of_range THEN
      RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid native batch request';
    END;
  END LOOP;
  BEGIN
    EXECUTE 'SELECT pgrdf.version(),pgrdf.build_id()' INTO native_version,native_build;
    SELECT extversion INTO extension_version FROM pg_catalog.pg_extension WHERE extname='pgrdf';
    WITH expected(relation_name,column_name,type_oid,relation_kind,not_null) AS (VALUES
      ('_pgrdf_dictionary','id','pg_catalog.int8'::regtype::oid,'r',true),
      ('_pgrdf_dictionary','term_type','pg_catalog.int2'::regtype::oid,'r',true),
      ('_pgrdf_dictionary','lexical_value','pg_catalog.text'::regtype::oid,'r',true),
      ('_pgrdf_dictionary','datatype_iri_id','pg_catalog.int8'::regtype::oid,'r',false),
      ('_pgrdf_dictionary','language_tag','pg_catalog.text'::regtype::oid,'r',false),
      ('_pgrdf_quads','subject_id','pg_catalog.int8'::regtype::oid,'p',true),
      ('_pgrdf_quads','predicate_id','pg_catalog.int8'::regtype::oid,'p',true),
      ('_pgrdf_quads','object_id','pg_catalog.int8'::regtype::oid,'p',true),
      ('_pgrdf_quads','graph_id','pg_catalog.int8'::regtype::oid,'p',true),
      ('_pgrdf_graphs','graph_id','pg_catalog.int8'::regtype::oid,'r',true),
      ('_pgrdf_graphs','iri','pg_catalog.text'::regtype::oid,'r',true))
    SELECT count(*)=11 INTO compatible FROM expected e
      JOIN pg_catalog.pg_namespace n ON n.nspname='pgrdf'
      JOIN pg_catalog.pg_class c ON c.relnamespace=n.oid AND c.relname=e.relation_name
        AND c.relkind::text=e.relation_kind
      JOIN pg_catalog.pg_attribute a ON a.attrelid=c.oid AND a.attname=e.column_name AND NOT a.attisdropped
        AND a.atttypid=e.type_oid AND a.attnotnull=e.not_null;
  EXCEPTION WHEN undefined_function OR undefined_table OR undefined_column THEN
    RAISE EXCEPTION USING ERRCODE='55000',MESSAGE='unsupported durable knowledge native batch engine';
  END;
  IF native_version IS DISTINCT FROM '0.6.34' OR native_build IS DISTINCT FROM 'v0.6.34'
     OR extension_version IS DISTINCT FROM '0.6.34' OR compatible IS DISTINCT FROM true THEN
    RAISE EXCEPTION USING ERRCODE='55000',MESSAGE='unsupported durable knowledge native batch engine';
  END IF;

  -- Enumerate graph-local subject IDs once. No dictionary-wide prefix scans.
  -- Scope roots retain arbitrary descendants, including subjects absent from expected RDF.
  FOR selected IN
    WITH requests AS MATERIALIZED (
      SELECT ord,(v->>'unit_id')::uuid AS uid,(v->>'revision')::bigint AS rev,
             (v->>'event_id')::uuid AS eid,(v->>'include_revision')::boolean AS inc
      FROM pg_catalog.jsonb_array_elements(p_requests) WITH ORDINALITY AS input(v,ord)
    ), refs AS MATERIALIZED (
      SELECT *, 'urn:tect:dk:unit:'||p_tenant||':'||p_workspace||':'||uid AS uiri,
        'urn:tect:dk:unit:'||p_tenant||':'||p_workspace||':'||uid||':revision:'||rev AS riri,
        'urn:tect:dk:event:'||p_tenant||':'||p_workspace||':'||eid AS eiri,
        'urn:tect:dk:unit:'||p_tenant||':'||p_workspace||':'||uid||':event:'||eid AS ciri
      FROM requests
    ), roots AS MATERIALIZED (
      SELECT ord,uiri AS root,true AS unit_root FROM refs WHERE inc
      UNION ALL SELECT ord,riri,false FROM refs WHERE inc
      UNION ALL SELECT ord,eiri,false FROM refs
      UNION ALL SELECT ord,ciri,false FROM refs
    ), assertions AS MATERIALIZED (
      SELECT DISTINCT r.ord,a->>'subject_iri' AS subject_iri,
        CASE a->>'predicate' WHEN 'broader_concept' THEN 'urn:tect:dk:v2:broaderConcept'
          WHEN 'classified_as' THEN 'urn:tect:dk:v2:classifiedAs'
          WHEN 'has_environment' THEN 'urn:tect:dk:v2:hasEnvironment'
          WHEN 'applies_to' THEN 'urn:tect:dk:v2:appliesTo' END AS predicate_iri,
        a->>'object_iri' AS object_iri
      FROM refs r JOIN public.knowledge_revisions kr
        ON kr.tenant_id=p_tenant AND kr.workspace_id=p_workspace AND kr.unit_id=r.uid AND kr.revision=r.rev
      CROSS JOIN LATERAL pg_catalog.jsonb_array_elements(
        COALESCE(kr.document_payload->'graph_assertions','[]'::jsonb)) AS a
      WHERE r.inc AND NOT kr.payload_erased
    ), graph AS MATERIALIZED (
      SELECT g.graph_id FROM pgrdf._pgrdf_graphs g
      WHERE g.iri='urn:tect:dk:workspace:'||p_tenant||':'||p_workspace
    ), subject_ids AS MATERIALIZED (
      SELECT DISTINCT q.subject_id FROM pgrdf._pgrdf_quads q
      WHERE q.graph_id=(SELECT g.graph_id FROM graph g)
    ), subjects AS MATERIALIZED (
      SELECT ids.subject_id,d.term_type,d.lexical_value,d.datatype_iri_id,d.language_tag,
        pg_catalog.string_to_array(d.lexical_value,':') AS parts
      FROM subject_ids ids LEFT JOIN pgrdf._pgrdf_dictionary d ON d.id=ids.subject_id
    ), scopes AS MATERIALIZED (
      SELECT s.*, CASE
        WHEN parts[1:3]=ARRAY['urn','tect','dk'] AND parts[4]='unit' AND pg_catalog.cardinality(parts)=7
          THEN lexical_value
        WHEN parts[1:3]=ARRAY['urn','tect','dk'] AND parts[4]='unit' AND parts[8] IN ('revision','event')
          AND pg_catalog.cardinality(parts)>=9 THEN pg_catalog.array_to_string(parts[1:9],':')
        WHEN parts[1:3]=ARRAY['urn','tect','dk'] AND parts[4]='event' AND pg_catalog.cardinality(parts)=7
          THEN lexical_value END AS root
      FROM subjects s
    ), owned_subjects AS MATERIALIZED (
      SELECT roots.ord,roots.unit_root,s.subject_id FROM scopes s JOIN roots
        ON s.root COLLATE "C"=roots.root COLLATE "C"
      -- Retain unsupported scoped subjects for the fail-closed bad_kind check below.
      -- Valid blank-node filtering remains subject to paired scalar qualification.
      WHERE s.term_type NOT IN (1,2,3) OR s.term_type=1 OR (s.term_type=3 AND NOT roots.unit_root
        AND pg_catalog.starts_with(s.lexical_value,roots.root||':'))
    ), assertion_subjects AS MATERIALIZED (
      SELECT a.*,s.subject_id FROM assertions a JOIN subjects s
        ON s.lexical_value COLLATE "C"=a.subject_iri COLLATE "C" AND s.term_type=1
    ), needed AS MATERIALIZED (
      SELECT subject_id FROM owned_subjects UNION SELECT subject_id FROM assertion_subjects
    ), quads AS MATERIALIZED (
      SELECT q.subject_id,s.term_type AS st,s.lexical_value AS sv,
        sd.lexical_value AS sdv,s.language_tag AS slang,
        p.term_type AS pt,p.lexical_value AS pv,pd.lexical_value AS pdv,p.language_tag AS plang,
        o.term_type AS ot,o.lexical_value AS ov,
        o.datatype_iri_id AS dtid,dt.lexical_value AS dv,o.language_tag AS lang,
        p.id IS NULL OR o.id IS NULL OR (o.datatype_iri_id IS NOT NULL AND dt.id IS NULL)
          OR (s.datatype_iri_id IS NOT NULL AND sd.id IS NULL)
          OR (p.datatype_iri_id IS NOT NULL AND pd.id IS NULL) AS broken
      FROM needed n JOIN pgrdf._pgrdf_quads q
        ON q.graph_id=(SELECT g.graph_id FROM graph g) AND q.subject_id=n.subject_id
      JOIN subjects s ON s.subject_id=q.subject_id
      LEFT JOIN pgrdf._pgrdf_dictionary p ON p.id=q.predicate_id
      LEFT JOIN pgrdf._pgrdf_dictionary o ON o.id=q.object_id
      LEFT JOIN pgrdf._pgrdf_dictionary sd ON sd.id=s.datatype_iri_id
      LEFT JOIN pgrdf._pgrdf_dictionary pd ON pd.id=p.datatype_iri_id
      LEFT JOIN pgrdf._pgrdf_dictionary dt ON dt.id=o.datatype_iri_id
    ), selected_quads AS (
      SELECT os.ord,q.* FROM owned_subjects os JOIN quads q ON q.subject_id=os.subject_id
      JOIN refs r ON r.ord=os.ord
      WHERE (q.broken OR q.pv<>ALL(ARRAY['urn:tect:dk:v2:broaderConcept','urn:tect:dk:v2:classifiedAs',
                   'urn:tect:dk:v2:hasEnvironment','urn:tect:dk:v2:appliesTo']))
        AND (q.broken OR NOT os.unit_root OR (q.pt=1 AND q.ot=1 AND
          ((q.pv='http://www.w3.org/1999/02/22-rdf-syntax-ns#type' AND q.ov='urn:tect:dk:KnowledgeUnit')
           OR (q.pv='urn:tect:dk:hasRevision' AND q.ov=r.riri))))
      UNION ALL
      SELECT a.ord,q.* FROM assertion_subjects a JOIN quads q ON q.subject_id=a.subject_id
      WHERE q.broken OR (q.st=1 AND q.pt=1 AND q.ot=1
        AND q.pv COLLATE "C"=a.predicate_iri COLLATE "C" AND q.ov COLLATE "C"=a.object_iri COLLATE "C")
    ), encoded AS (
      SELECT DISTINCT q.ord,q.broken,
        q.st NOT IN (1,2,3) OR q.pt NOT IN (1,2,3) OR q.ot NOT IN (1,2,3) AS bad_kind,
        pg_catalog.jsonb_build_object(
          'subject',pg_catalog.jsonb_strip_nulls(pg_catalog.jsonb_build_object(
            'type',CASE q.st WHEN 1 THEN 'iri' WHEN 2 THEN 'bnode' WHEN 3 THEN 'literal' END,
            'value',q.sv,'datatype',COALESCE(q.sdv,CASE WHEN q.st=3 AND q.slang IS NOT NULL
              THEN 'http://www.w3.org/1999/02/22-rdf-syntax-ns#langString' END),'language',q.slang)),
          'predicate',pg_catalog.jsonb_strip_nulls(pg_catalog.jsonb_build_object(
            'type',CASE q.pt WHEN 1 THEN 'iri' WHEN 2 THEN 'bnode' WHEN 3 THEN 'literal' END,
            'value',q.pv,'datatype',COALESCE(q.pdv,CASE WHEN q.pt=3 AND q.plang IS NOT NULL
              THEN 'http://www.w3.org/1999/02/22-rdf-syntax-ns#langString' END),'language',q.plang)),
          'object',pg_catalog.jsonb_strip_nulls(pg_catalog.jsonb_build_object(
            'type',CASE q.ot WHEN 1 THEN 'iri' WHEN 2 THEN 'bnode' WHEN 3 THEN 'literal' END,
            'value',q.ov,'datatype',COALESCE(q.dv,CASE WHEN q.ot=3 AND q.lang IS NOT NULL
              THEN 'http://www.w3.org/1999/02/22-rdf-syntax-ns#langString' END),'language',q.lang))) AS row
      FROM selected_quads q
    )
    SELECT r.*,e.row,e.broken,e.bad_kind,
      EXISTS(SELECT 1 FROM subjects s WHERE s.term_type IS NULL) AS missing_subject,
      EXISTS(SELECT 1 FROM owned_subjects os JOIN subjects s ON s.subject_id=os.subject_id
             WHERE s.term_type NOT IN (1,2,3)) AS unsupported_subject
    FROM refs r LEFT JOIN encoded e ON e.ord=r.ord
  LOOP
    IF selected.missing_subject OR selected.unsupported_subject OR selected.broken OR selected.bad_kind THEN
      RAISE EXCEPTION USING ERRCODE='55000',MESSAGE='invalid durable knowledge native batch term';
    END IF;
    request_ordinal:=selected.ord; unit_id:=selected.uid; revision:=selected.rev;
    event_id:=selected.eid; include_revision:=selected.inc; triple:=selected.row;
    RETURN NEXT;
  END LOOP;
END
$fn$;

CREATE FUNCTION public.tect_dk2_native_read_batch(p_tenant uuid,p_workspace uuid,p_requests jsonb)
RETURNS TABLE(request_ordinal bigint,unit_id uuid,revision bigint,event_id uuid,
              include_revision boolean,triple jsonb)
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public
AS $fn$
DECLARE caller_is_admin boolean;
BEGIN
  SELECT pg_catalog.pg_has_role(SESSION_USER,d.datdba,'MEMBER') INTO caller_is_admin
    FROM pg_catalog.pg_database d WHERE d.datname=pg_catalog.current_database();
  IF caller_is_admin IS DISTINCT FROM true AND public.tect_dk_database_identity_ready() IS DISTINCT FROM true THEN
    RAISE EXCEPTION USING ERRCODE='55000',MESSAGE='durable knowledge recovery required';
  END IF;
  RETURN QUERY SELECT * FROM public.tect_dk2_internal_native_read_batch(p_tenant,p_workspace,p_requests);
END
$fn$;

REVOKE ALL PRIVILEGES ON FUNCTION public.tect_dk2_native_read_batch(uuid,uuid,jsonb),
  public.tect_dk2_internal_native_read_batch(uuid,uuid,jsonb) FROM PUBLIC;

-- Existing runtime identities inherit only the public surface's execute grant.
-- Fresh-role provisioning must grant this surface explicitly alongside the scalar reader.
DO $grant$
DECLARE recipient record;
BEGIN
  FOR recipient IN
    SELECT DISTINCT r.rolname FROM pg_catalog.pg_proc p
    CROSS JOIN LATERAL pg_catalog.aclexplode(p.proacl) a
    JOIN pg_catalog.pg_roles r ON r.oid=a.grantee
    WHERE p.oid='public.tect_dk2_native_read(uuid,uuid,uuid,bigint,uuid,boolean)'::regprocedure
      AND a.privilege_type='EXECUTE' AND r.oid<>p.proowner
  LOOP
    EXECUTE pg_catalog.format('GRANT EXECUTE ON FUNCTION public.tect_dk2_native_read_batch(uuid,uuid,jsonb) TO %I',recipient.rolname);
  END LOOP;
END
$grant$;
