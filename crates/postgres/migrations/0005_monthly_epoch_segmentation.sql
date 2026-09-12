-- Epoch is a derived UTC calendar-month key. It never changes lifecycle state.
CREATE FUNCTION public.tect_preserve_created_at()
RETURNS trigger
LANGUAGE plpgsql
SET search_path = pg_catalog, public
AS $function$
BEGIN
    IF NEW.created_at IS DISTINCT FROM OLD.created_at THEN
        RAISE EXCEPTION 'created_at is immutable'
            USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END
$function$;

REVOKE ALL PRIVILEGES ON FUNCTION public.tect_preserve_created_at() FROM PUBLIC;

ALTER TABLE workspaces
    ADD COLUMN epoch_month date GENERATED ALWAYS AS (
        pg_catalog.date_trunc('month', created_at AT TIME ZONE 'UTC')::date
    ) STORED NOT NULL;
ALTER TABLE agent_sessions
    ADD COLUMN epoch_month date GENERATED ALWAYS AS (
        pg_catalog.date_trunc('month', created_at AT TIME ZONE 'UTC')::date
    ) STORED NOT NULL;
ALTER TABLE workspace_events
    ADD COLUMN epoch_month date GENERATED ALWAYS AS (
        pg_catalog.date_trunc('month', created_at AT TIME ZONE 'UTC')::date
    ) STORED NOT NULL;
ALTER TABLE source_repositories
    ADD COLUMN epoch_month date GENERATED ALWAYS AS (
        pg_catalog.date_trunc('month', created_at AT TIME ZONE 'UTC')::date
    ) STORED NOT NULL;
ALTER TABLE source_worktrees
    ADD COLUMN epoch_month date GENERATED ALWAYS AS (
        pg_catalog.date_trunc('month', created_at AT TIME ZONE 'UTC')::date
    ) STORED NOT NULL;
ALTER TABLE session_worktrees
    ADD COLUMN epoch_month date GENERATED ALWAYS AS (
        pg_catalog.date_trunc('month', created_at AT TIME ZONE 'UTC')::date
    ) STORED NOT NULL;
ALTER TABLE programs
    ADD COLUMN epoch_month date GENERATED ALWAYS AS (
        pg_catalog.date_trunc('month', created_at AT TIME ZONE 'UTC')::date
    ) STORED NOT NULL;
ALTER TABLE program_inputs
    ADD COLUMN epoch_month date GENERATED ALWAYS AS (
        pg_catalog.date_trunc('month', created_at AT TIME ZONE 'UTC')::date
    ) STORED NOT NULL;
ALTER TABLE setup_session_directories
    ADD COLUMN epoch_month date GENERATED ALWAYS AS (
        pg_catalog.date_trunc('month', created_at AT TIME ZONE 'UTC')::date
    ) STORED NOT NULL;
ALTER TABLE workspace_setups
    ADD COLUMN epoch_month date GENERATED ALWAYS AS (
        pg_catalog.date_trunc('month', created_at AT TIME ZONE 'UTC')::date
    ) STORED NOT NULL;
ALTER TABLE workspace_setup_inputs
    ADD COLUMN epoch_month date GENERATED ALWAYS AS (
        pg_catalog.date_trunc('month', created_at AT TIME ZONE 'UTC')::date
    ) STORED NOT NULL;

CREATE TRIGGER workspaces_created_at_immutable
    BEFORE UPDATE ON workspaces FOR EACH ROW
    EXECUTE FUNCTION public.tect_preserve_created_at();
CREATE TRIGGER agent_sessions_created_at_immutable
    BEFORE UPDATE ON agent_sessions FOR EACH ROW
    EXECUTE FUNCTION public.tect_preserve_created_at();
CREATE TRIGGER workspace_events_created_at_immutable
    BEFORE UPDATE ON workspace_events FOR EACH ROW
    EXECUTE FUNCTION public.tect_preserve_created_at();
CREATE TRIGGER source_repositories_created_at_immutable
    BEFORE UPDATE ON source_repositories FOR EACH ROW
    EXECUTE FUNCTION public.tect_preserve_created_at();
CREATE TRIGGER source_worktrees_created_at_immutable
    BEFORE UPDATE ON source_worktrees FOR EACH ROW
    EXECUTE FUNCTION public.tect_preserve_created_at();
CREATE TRIGGER session_worktrees_created_at_immutable
    BEFORE UPDATE ON session_worktrees FOR EACH ROW
    EXECUTE FUNCTION public.tect_preserve_created_at();
CREATE TRIGGER programs_created_at_immutable
    BEFORE UPDATE ON programs FOR EACH ROW
    EXECUTE FUNCTION public.tect_preserve_created_at();
CREATE TRIGGER program_inputs_created_at_immutable
    BEFORE UPDATE ON program_inputs FOR EACH ROW
    EXECUTE FUNCTION public.tect_preserve_created_at();
CREATE TRIGGER setup_session_directories_created_at_immutable
    BEFORE UPDATE ON setup_session_directories FOR EACH ROW
    EXECUTE FUNCTION public.tect_preserve_created_at();
CREATE TRIGGER workspace_setups_created_at_immutable
    BEFORE UPDATE ON workspace_setups FOR EACH ROW
    EXECUTE FUNCTION public.tect_preserve_created_at();
CREATE TRIGGER workspace_setup_inputs_created_at_immutable
    BEFORE UPDATE ON workspace_setup_inputs FOR EACH ROW
    EXECUTE FUNCTION public.tect_preserve_created_at();

CREATE INDEX workspaces_epoch_idx
    ON workspaces (tenant_id, epoch_month, created_at, id);
CREATE INDEX agent_sessions_epoch_idx
    ON agent_sessions (tenant_id, workspace_id, epoch_month, created_at, id);
CREATE INDEX workspace_events_epoch_idx
    ON workspace_events (tenant_id, workspace_id, epoch_month, created_at, id);
CREATE INDEX source_repositories_epoch_idx
    ON source_repositories (tenant_id, workspace_id, epoch_month, created_at, id);
CREATE INDEX source_worktrees_epoch_idx
    ON source_worktrees (tenant_id, workspace_id, epoch_month, created_at, id);
CREATE INDEX session_worktrees_epoch_idx
    ON session_worktrees (
        tenant_id, workspace_id, epoch_month, created_at, session_id, worktree_id
    );
CREATE INDEX programs_epoch_idx
    ON programs (tenant_id, workspace_id, epoch_month, created_at, id);
CREATE INDEX program_inputs_epoch_idx
    ON program_inputs (
        tenant_id, workspace_id, epoch_month, created_at, program_id, sequence
    );
CREATE INDEX setup_session_directories_epoch_idx
    ON setup_session_directories (
        tenant_id, workspace_id, epoch_month, created_at, host_id, session_id
    );
CREATE INDEX workspace_setups_epoch_idx
    ON workspace_setups (
        tenant_id, workspace_id, epoch_month, created_at, host_id, id
    );
CREATE INDEX workspace_setup_inputs_epoch_idx
    ON workspace_setup_inputs (
        tenant_id, workspace_id, epoch_month, created_at, setup_id, sequence
    );
