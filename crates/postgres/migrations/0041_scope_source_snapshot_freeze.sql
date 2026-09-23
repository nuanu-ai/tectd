-- Serialize source-ref insertion with publication and manifest preparation.
-- The runtime role already has SELECT on both tables; these are invoker functions.
CREATE FUNCTION tect_scope_source_ref_before_insert() RETURNS trigger
LANGUAGE plpgsql SET search_path = pg_catalog, public AS $$
DECLARE
    published_snapshot uuid;
    published_sequence bigint;
    target_sequence bigint;
BEGIN
    SELECT current_snapshot_id INTO published_snapshot
      FROM public.scope_candidate_sets
     WHERE tenant_id=NEW.tenant_id AND workspace_id=NEW.workspace_id
       AND id=NEW.candidate_set_id FOR SHARE;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'source reference candidate set does not exist';
    END IF;

    SELECT sequence INTO target_sequence FROM public.scope_candidate_snapshots
     WHERE tenant_id=NEW.tenant_id AND workspace_id=NEW.workspace_id
       AND candidate_set_id=NEW.candidate_set_id AND id=NEW.snapshot_id;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'source reference snapshot does not belong to candidate set';
    END IF;

    IF published_snapshot IS NOT NULL THEN
        SELECT sequence INTO published_sequence FROM public.scope_candidate_snapshots
         WHERE tenant_id=NEW.tenant_id AND workspace_id=NEW.workspace_id
           AND candidate_set_id=NEW.candidate_set_id AND id=published_snapshot;
        IF target_sequence <= published_sequence THEN
            RAISE EXCEPTION 'cannot append source reference to published snapshot';
        END IF;
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER scope_candidate_source_refs_freeze
BEFORE INSERT ON scope_candidate_source_refs
FOR EACH ROW EXECUTE FUNCTION tect_scope_source_ref_before_insert();

CREATE FUNCTION tect_scope_candidate_set_snapshot_forward() RETURNS trigger
LANGUAGE plpgsql SET search_path = pg_catalog, public AS $$
DECLARE
    old_sequence bigint;
    new_sequence bigint;
BEGIN
    IF NEW.current_snapshot_id IS NOT DISTINCT FROM OLD.current_snapshot_id THEN
        RETURN NEW;
    END IF;
    IF OLD.current_snapshot_id IS NOT NULL THEN
        IF NEW.current_snapshot_id IS NULL THEN
            RAISE EXCEPTION 'cannot clear published candidate snapshot';
        END IF;
        SELECT sequence INTO old_sequence FROM public.scope_candidate_snapshots
         WHERE tenant_id=OLD.tenant_id AND workspace_id=OLD.workspace_id
           AND candidate_set_id=OLD.id AND id=OLD.current_snapshot_id;
        SELECT sequence INTO new_sequence FROM public.scope_candidate_snapshots
         WHERE tenant_id=NEW.tenant_id AND workspace_id=NEW.workspace_id
           AND candidate_set_id=NEW.id AND id=NEW.current_snapshot_id;
        IF new_sequence IS NULL OR new_sequence <= old_sequence THEN
            RAISE EXCEPTION 'candidate snapshot must advance to a newer sequence in the same set';
        END IF;
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER scope_candidate_sets_snapshot_forward
BEFORE UPDATE OF current_snapshot_id ON scope_candidate_sets
FOR EACH ROW EXECUTE FUNCTION tect_scope_candidate_set_snapshot_forward();

REVOKE ALL ON FUNCTION tect_scope_source_ref_before_insert() FROM PUBLIC;
REVOKE ALL ON FUNCTION tect_scope_candidate_set_snapshot_forward() FROM PUBLIC;
