-- Complete the additive WP5 graph projection without changing the legacy
-- snapshot/v0.6 storage contract.
ALTER TABLE scope_candidate_delta_candidates
    ADD COLUMN title text,
    ADD COLUMN outcome text;
UPDATE scope_candidate_delta_candidates
SET title=COALESCE(NULLIF(payload->>'title',''),'legacy candidate')
WHERE title IS NULL;
ALTER TABLE scope_candidate_delta_candidates
    ALTER COLUMN title SET NOT NULL,
    ADD CONSTRAINT scope_candidate_delta_candidates_title_check
        CHECK (pg_catalog.length(pg_catalog.btrim(title)) > 0);

ALTER TABLE scope_candidate_delta_goals
    ADD COLUMN text text,
    ADD COLUMN finite boolean NOT NULL DEFAULT true,
    ADD COLUMN resolved boolean NOT NULL DEFAULT false,
    ADD COLUMN source_ref_id uuid;
UPDATE scope_candidate_delta_goals
SET text=COALESCE(NULLIF(payload->>'text',''),'legacy goal')
WHERE text IS NULL;
ALTER TABLE scope_candidate_delta_goals
    ALTER COLUMN text SET NOT NULL,
    ADD CONSTRAINT scope_candidate_delta_goals_text_check
        CHECK (pg_catalog.length(pg_catalog.btrim(text)) > 0),
    ADD CONSTRAINT scope_candidate_delta_goals_source_fk
        FOREIGN KEY (tenant_id,workspace_id,candidate_set_id,source_ref_id)
        REFERENCES scope_candidate_source_refs
            (tenant_id,workspace_id,candidate_set_id,id);

ALTER TABLE scope_candidate_delta_coverage
    ADD CONSTRAINT scope_candidate_delta_coverage_candidate_fk
        FOREIGN KEY (tenant_id,workspace_id,candidate_set_id,candidate_id)
        REFERENCES scope_candidate_delta_candidates
            (tenant_id,workspace_id,candidate_set_id,candidate_id),
    ADD CONSTRAINT scope_candidate_delta_coverage_goal_fk
        FOREIGN KEY (tenant_id,workspace_id,candidate_set_id,goal_id)
        REFERENCES scope_candidate_delta_goals
            (tenant_id,workspace_id,candidate_set_id,goal_id);

CREATE TABLE scope_candidate_delta_evidence (
    tenant_id uuid NOT NULL,
    workspace_id uuid NOT NULL,
    candidate_set_id uuid NOT NULL,
    evidence_id uuid NOT NULL,
    candidate_id uuid,
    goal_id uuid,
    source_ref_id uuid NOT NULL,
    revision bigint NOT NULL DEFAULT 1,
    summary text NOT NULL,
    deleted boolean NOT NULL DEFAULT false,
    created_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
    PRIMARY KEY (tenant_id,workspace_id,candidate_set_id,evidence_id),
    CONSTRAINT scope_candidate_delta_evidence_target_check
        CHECK ((candidate_id IS NOT NULL)::integer + (goal_id IS NOT NULL)::integer = 1),
    CONSTRAINT scope_candidate_delta_evidence_revision_check CHECK (revision >= 1),
    CONSTRAINT scope_candidate_delta_evidence_summary_check
        CHECK (pg_catalog.length(pg_catalog.btrim(summary)) > 0),
    CONSTRAINT scope_candidate_delta_evidence_candidate_fk
        FOREIGN KEY (tenant_id,workspace_id,candidate_set_id,candidate_id)
        REFERENCES scope_candidate_delta_candidates
            (tenant_id,workspace_id,candidate_set_id,candidate_id),
    CONSTRAINT scope_candidate_delta_evidence_goal_fk
        FOREIGN KEY (tenant_id,workspace_id,candidate_set_id,goal_id)
        REFERENCES scope_candidate_delta_goals
            (tenant_id,workspace_id,candidate_set_id,goal_id),
    CONSTRAINT scope_candidate_delta_evidence_source_fk
        FOREIGN KEY (tenant_id,workspace_id,candidate_set_id,source_ref_id)
        REFERENCES scope_candidate_source_refs
            (tenant_id,workspace_id,candidate_set_id,id)
);

CREATE TABLE scope_candidate_delta_blockers (
    tenant_id uuid NOT NULL,
    workspace_id uuid NOT NULL,
    candidate_set_id uuid NOT NULL,
    blocker_id uuid NOT NULL,
    goal_id uuid NOT NULL,
    source_ref_id uuid NOT NULL,
    revision bigint NOT NULL DEFAULT 1,
    summary text NOT NULL,
    deleted boolean NOT NULL DEFAULT false,
    created_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
    PRIMARY KEY (tenant_id,workspace_id,candidate_set_id,blocker_id),
    CONSTRAINT scope_candidate_delta_blockers_revision_check CHECK (revision >= 1),
    CONSTRAINT scope_candidate_delta_blockers_summary_check
        CHECK (pg_catalog.length(pg_catalog.btrim(summary)) > 0),
    CONSTRAINT scope_candidate_delta_blockers_goal_fk
        FOREIGN KEY (tenant_id,workspace_id,candidate_set_id,goal_id)
        REFERENCES scope_candidate_delta_goals
            (tenant_id,workspace_id,candidate_set_id,goal_id),
    CONSTRAINT scope_candidate_delta_blockers_source_fk
        FOREIGN KEY (tenant_id,workspace_id,candidate_set_id,source_ref_id)
        REFERENCES scope_candidate_source_refs
            (tenant_id,workspace_id,candidate_set_id,id)
);

CREATE TABLE scope_candidate_delta_supersessions (
    tenant_id uuid NOT NULL,
    workspace_id uuid NOT NULL,
    candidate_set_id uuid NOT NULL,
    candidate_id uuid NOT NULL,
    replacement_candidate_id uuid NOT NULL,
    created_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
    PRIMARY KEY (tenant_id,workspace_id,candidate_set_id,candidate_id),
    CONSTRAINT scope_candidate_delta_supersessions_distinct_check
        CHECK (candidate_id <> replacement_candidate_id),
    CONSTRAINT scope_candidate_delta_supersessions_candidate_fk
        FOREIGN KEY (tenant_id,workspace_id,candidate_set_id,candidate_id)
        REFERENCES scope_candidate_delta_candidates
            (tenant_id,workspace_id,candidate_set_id,candidate_id),
    CONSTRAINT scope_candidate_delta_supersessions_replacement_fk
        FOREIGN KEY (tenant_id,workspace_id,candidate_set_id,replacement_candidate_id)
        REFERENCES scope_candidate_delta_candidates
            (tenant_id,workspace_id,candidate_set_id,candidate_id)
);

CREATE INDEX scope_candidate_delta_coverage_goal_idx
    ON scope_candidate_delta_coverage
        (tenant_id,workspace_id,candidate_set_id,goal_id,candidate_id);
CREATE INDEX scope_candidate_delta_evidence_candidate_idx
    ON scope_candidate_delta_evidence
        (tenant_id,workspace_id,candidate_set_id,candidate_id) WHERE NOT deleted;
CREATE INDEX scope_candidate_delta_evidence_goal_idx
    ON scope_candidate_delta_evidence
        (tenant_id,workspace_id,candidate_set_id,goal_id) WHERE NOT deleted;
CREATE INDEX scope_candidate_delta_blockers_goal_idx
    ON scope_candidate_delta_blockers
        (tenant_id,workspace_id,candidate_set_id,goal_id) WHERE NOT deleted;
CREATE INDEX scope_candidate_delta_supersessions_replacement_idx
    ON scope_candidate_delta_supersessions
        (tenant_id,workspace_id,candidate_set_id,replacement_candidate_id);

ALTER TABLE scope_candidate_delta_evidence ENABLE ROW LEVEL SECURITY;
ALTER TABLE scope_candidate_delta_evidence FORCE ROW LEVEL SECURITY;
ALTER TABLE scope_candidate_delta_blockers ENABLE ROW LEVEL SECURITY;
ALTER TABLE scope_candidate_delta_blockers FORCE ROW LEVEL SECURITY;
ALTER TABLE scope_candidate_delta_supersessions ENABLE ROW LEVEL SECURITY;
ALTER TABLE scope_candidate_delta_supersessions FORCE ROW LEVEL SECURITY;

CREATE POLICY scope_candidate_delta_evidence_tenant_scope
    ON scope_candidate_delta_evidence
    USING (tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid)
    WITH CHECK (tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid);
CREATE POLICY scope_candidate_delta_blockers_tenant_scope
    ON scope_candidate_delta_blockers
    USING (tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid)
    WITH CHECK (tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid);
CREATE POLICY scope_candidate_delta_supersessions_tenant_scope
    ON scope_candidate_delta_supersessions
    USING (tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid)
    WITH CHECK (tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid);

REVOKE ALL PRIVILEGES ON TABLE
    scope_candidate_delta_evidence,
    scope_candidate_delta_blockers,
    scope_candidate_delta_supersessions
FROM PUBLIC;
