-- Legacy links from migration 0068 have no attested node mapping. A new link
-- must carry a nonempty mapping in the same immutable row as its receipt FK.
ALTER TABLE matrix_planning_selection_links
    ADD COLUMN mapped_nodes jsonb;

-- NOT VALID preserves old links as distinguishable, unattestable NULL rows,
-- while PostgreSQL enforces this constraint on every future INSERT.
ALTER TABLE matrix_planning_selection_links
    ADD CONSTRAINT matrix_planning_selection_mapped_nodes_required
    CHECK (CASE WHEN pg_catalog.jsonb_typeof(mapped_nodes) = 'array'
        THEN pg_catalog.jsonb_array_length(mapped_nodes) BETWEEN 1 AND 100
        ELSE false END) NOT VALID;

-- Full ordered node identity is checked against the same immutable save receipt.
CREATE FUNCTION public.matrix_planning_selection_require_mapped_nodes() RETURNS trigger
LANGUAGE plpgsql VOLATILE SECURITY DEFINER SET search_path=pg_catalog,public,pg_temp
AS $mapped_nodes$
DECLARE request jsonb; result jsonb; node jsonb; submitted jsonb; saved jsonb;
        indices jsonb := '[]'::jsonb; position bigint; previous bigint := -1;
        revision bigint; node_id uuid;
BEGIN
    IF pg_catalog.current_setting('transaction_isolation') <> 'read committed' THEN
        RAISE EXCEPTION 'Matrix V2 planning requires READ COMMITTED'
            USING ERRCODE = '0A000';
    END IF;
    SELECT request_payload,result_payload INTO request,result
      FROM public.native_planning_receipts
     WHERE (tenant_id,workspace_id,entity_id,operation,request_id)=
           (NEW.tenant_id,NEW.workspace_id,NEW.candidate_set_id,NEW.operation,NEW.caller_request_id)
       AND NOT payload_erased
     FOR SHARE;
    IF NOT FOUND OR request IS NULL OR result IS NULL
       OR pg_catalog.jsonb_typeof(NEW.mapped_nodes) IS DISTINCT FROM 'array'
       OR pg_catalog.jsonb_typeof(request#>'{draft,nodes}') IS DISTINCT FROM 'array'
       OR pg_catalog.jsonb_typeof(result#>'{draft,nodes}') IS DISTINCT FROM 'array' THEN
        RAISE EXCEPTION 'Matrix selection lacks mapped receipt nodes' USING ERRCODE='23514';
    END IF;
    IF pg_catalog.jsonb_array_length(NEW.mapped_nodes) NOT BETWEEN 1 AND 100
       OR pg_catalog.jsonb_array_length(request#>'{draft,nodes}') <>
          pg_catalog.jsonb_array_length(result#>'{draft,nodes}') THEN
        RAISE EXCEPTION 'Matrix mapped receipt node count mismatch' USING ERRCODE='23514';
    END IF;
    FOR node IN SELECT value FROM pg_catalog.jsonb_array_elements(NEW.mapped_nodes) LOOP
        IF pg_catalog.jsonb_typeof(node) IS DISTINCT FROM 'object'
           OR NOT (node ?& ARRAY['draft_index','node_id','node_revision'])
           OR node - ARRAY['draft_index','node_id','node_revision'] <> '{}'::jsonb
           OR pg_catalog.jsonb_typeof(node->'draft_index') IS DISTINCT FROM 'number'
           OR pg_catalog.jsonb_typeof(node->'node_revision') IS DISTINCT FROM 'number'
           OR node->>'draft_index' !~ '^[0-9]+$'
           OR node->>'node_revision' !~ '^[1-9][0-9]*$'
           OR pg_catalog.jsonb_typeof(node->'node_id') IS DISTINCT FROM 'string'
           OR node->>'node_id' !~
              '^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$' THEN
            RAISE EXCEPTION 'Malformed Matrix mapped node' USING ERRCODE='23514';
        END IF;
        position := (node->>'draft_index')::bigint;
        revision := (node->>'node_revision')::bigint;
        node_id := (node->>'node_id')::uuid;
        IF position <= previous OR position >= pg_catalog.jsonb_array_length(request#>'{draft,nodes}')
           OR node_id='00000000-0000-0000-0000-000000000000'::uuid THEN
            RAISE EXCEPTION 'Invalid Matrix mapped index or identity' USING ERRCODE='23514';
        END IF;
        submitted := (request#>'{draft,nodes}')->position::integer;
        saved := (result#>'{draft,nodes}')->position::integer;
        IF submitted->>'kind' IS NULL OR submitted->>'kind' NOT IN ('work','decision')
           OR pg_catalog.jsonb_typeof(submitted->'identity') IS DISTINCT FROM 'object'
           OR submitted->'kind' IS DISTINCT FROM saved->'kind'
           OR saved->'id' IS DISTINCT FROM pg_catalog.to_jsonb(node_id)
           OR saved->'revision' IS DISTINCT FROM pg_catalog.to_jsonb(revision)
           OR (submitted#>'{identity,candidate_id}' IS NOT NULL
               AND submitted#>'{identity,candidate_id}' <> 'null'::jsonb
               AND submitted#>'{identity,candidate_id}' <> pg_catalog.to_jsonb(node_id)) THEN
            RAISE EXCEPTION 'Matrix mapped node differs from saved body' USING ERRCODE='23514';
        END IF;
        indices := indices || pg_catalog.jsonb_build_array(position);
        previous := position;
    END LOOP;
    IF request#>'{matrix_selection,mapped_draft_node_indices}' IS DISTINCT FROM indices THEN
        RAISE EXCEPTION 'Matrix mapped indices differ from selected receipt' USING ERRCODE='23514';
    END IF;
    RETURN NEW;
END
$mapped_nodes$;
CREATE TRIGGER matrix_planning_selection_mapped_nodes_guard
    BEFORE INSERT ON public.matrix_planning_selection_links
    FOR EACH ROW EXECUTE FUNCTION public.matrix_planning_selection_require_mapped_nodes();
REVOKE ALL PRIVILEGES ON FUNCTION public.matrix_planning_selection_require_mapped_nodes() FROM PUBLIC;
