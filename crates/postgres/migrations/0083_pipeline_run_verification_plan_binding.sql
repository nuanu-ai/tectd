-- Copy the selected plan only at the owner's explicit run begin. Historical
-- runs keep their original null binding; no advice or Slice open starts a run.
ALTER TABLE slice_pipeline_runs
    ADD COLUMN selected_option_id text,
    ADD COLUMN verification_plan_id text,
    ADD COLUMN verification_plan_version text,
    ADD COLUMN verification_plan_digest text,
    ADD CONSTRAINT slice_pipeline_run_plan_pair_check CHECK (
        (selected_option_id IS NULL AND verification_plan_id IS NULL
         AND verification_plan_version IS NULL AND verification_plan_digest IS NULL)
        OR COALESCE((selected_option_id IS NOT NULL
            AND verification_plan_id = 'verification-plan:' || verification_plan_digest
            AND selected_option_id = definition_kind || '+' || verification_plan_id
            AND pg_catalog.length(pg_catalog.btrim(verification_plan_version)) > 0
            AND verification_plan_digest ~ '^[0-9a-f]{64}$'), false)
    ) NOT VALID;

CREATE FUNCTION slice_pipeline_run_require_selected_plan() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, public
AS $run_plan$
DECLARE selected public.native_slices%ROWTYPE;
BEGIN
    IF TG_OP = 'UPDATE' THEN
        IF (NEW.selected_option_id,NEW.verification_plan_id,
            NEW.verification_plan_version,NEW.verification_plan_digest)
           IS DISTINCT FROM
           (OLD.selected_option_id,OLD.verification_plan_id,
            OLD.verification_plan_version,OLD.verification_plan_digest) THEN
            RAISE EXCEPTION 'pipeline run verification plan is immutable'
                USING ERRCODE='23514';
        END IF;
        RETURN NEW;
    END IF;

    SELECT * INTO selected FROM public.native_slices
    WHERE (tenant_id,workspace_id,id) =
          (NEW.tenant_id,NEW.workspace_id,NEW.slice_id) FOR SHARE;
    IF NOT FOUND
       OR (NEW.selected_option_id,NEW.verification_plan_id,
           NEW.verification_plan_version,NEW.verification_plan_digest)
          IS DISTINCT FROM
          (selected.selected_option_id,selected.verification_plan_id,
           selected.verification_plan_source_definition_version,
           selected.verification_plan_digest)
       OR (selected.verification_plan_id IS NOT NULL
           AND (NEW.definition_kind IS DISTINCT FROM selected.pipeline
                OR NEW.definition_version IS DISTINCT FROM
                   selected.verification_plan_source_definition_version
                OR NEW.definition_digest IS DISTINCT FROM
                   selected.verification_plan_source_definition_digest)) THEN
        RAISE EXCEPTION 'pipeline run does not match selected Slice verification plan'
            USING ERRCODE='23514';
    END IF;
    RETURN NEW;
END
$run_plan$;

CREATE TRIGGER slice_pipeline_run_selected_plan_guard
BEFORE INSERT OR UPDATE OF selected_option_id,verification_plan_id,
    verification_plan_version,verification_plan_digest ON slice_pipeline_runs
FOR EACH ROW EXECUTE FUNCTION slice_pipeline_run_require_selected_plan();
