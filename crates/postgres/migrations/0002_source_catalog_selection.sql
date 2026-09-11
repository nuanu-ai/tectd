ALTER TABLE agent_sessions
    ADD CONSTRAINT agent_sessions_scope_id_unique
    UNIQUE (tenant_id, workspace_id, host_id, id);

CREATE TABLE source_repositories (
    id uuid PRIMARY KEY,
    tenant_id uuid NOT NULL,
    workspace_id uuid NOT NULL,
    host_id uuid NOT NULL,
    common_dir text NOT NULL,
    created_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
    CONSTRAINT source_repositories_common_dir_check CHECK (
        pg_catalog.octet_length(common_dir) BETWEEN 1 AND 4096
    ),
    CONSTRAINT source_repositories_workspace_fk FOREIGN KEY (tenant_id, workspace_id)
        REFERENCES workspaces (tenant_id, id),
    CONSTRAINT source_repositories_host_fk FOREIGN KEY (tenant_id, host_id)
        REFERENCES hosts (tenant_id, id),
    CONSTRAINT source_repositories_scope_id_unique
        UNIQUE (tenant_id, workspace_id, host_id, id),
    CONSTRAINT source_repositories_common_dir_unique
        UNIQUE (tenant_id, workspace_id, host_id, common_dir)
);

CREATE TABLE source_worktrees (
    id uuid PRIMARY KEY,
    tenant_id uuid NOT NULL,
    workspace_id uuid NOT NULL,
    host_id uuid NOT NULL,
    repository_id uuid NOT NULL,
    path text NOT NULL,
    created_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
    CONSTRAINT source_worktrees_path_check CHECK (
        pg_catalog.octet_length(path) BETWEEN 1 AND 4096
    ),
    CONSTRAINT source_worktrees_repository_fk
        FOREIGN KEY (tenant_id, workspace_id, host_id, repository_id)
        REFERENCES source_repositories (tenant_id, workspace_id, host_id, id),
    CONSTRAINT source_worktrees_scope_id_unique
        UNIQUE (tenant_id, workspace_id, host_id, id),
    CONSTRAINT source_worktrees_path_unique
        UNIQUE (tenant_id, workspace_id, host_id, path)
);

CREATE TABLE session_worktrees (
    tenant_id uuid NOT NULL,
    workspace_id uuid NOT NULL,
    host_id uuid NOT NULL,
    session_id uuid NOT NULL,
    worktree_id uuid NOT NULL,
    created_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
    PRIMARY KEY (tenant_id, workspace_id, host_id, session_id, worktree_id),
    CONSTRAINT session_worktrees_session_fk
        FOREIGN KEY (tenant_id, workspace_id, host_id, session_id)
        REFERENCES agent_sessions (tenant_id, workspace_id, host_id, id),
    CONSTRAINT session_worktrees_worktree_fk
        FOREIGN KEY (tenant_id, workspace_id, host_id, worktree_id)
        REFERENCES source_worktrees (tenant_id, workspace_id, host_id, id)
);

ALTER TABLE source_repositories ENABLE ROW LEVEL SECURITY;
ALTER TABLE source_repositories FORCE ROW LEVEL SECURITY;
ALTER TABLE source_worktrees ENABLE ROW LEVEL SECURITY;
ALTER TABLE source_worktrees FORCE ROW LEVEL SECURITY;
ALTER TABLE session_worktrees ENABLE ROW LEVEL SECURITY;
ALTER TABLE session_worktrees FORCE ROW LEVEL SECURITY;

CREATE POLICY source_repositories_tenant_scope ON source_repositories
    USING (
        CURRENT_USER = pg_catalog.pg_get_userbyid(
            (SELECT relowner FROM pg_catalog.pg_class WHERE oid='public.source_repositories'::regclass)
        )
        OR tenant_id = NULLIF(pg_catalog.current_setting('tect.tenant_id', true), '')::uuid
    )
    WITH CHECK (
        CURRENT_USER = pg_catalog.pg_get_userbyid(
            (SELECT relowner FROM pg_catalog.pg_class WHERE oid='public.source_repositories'::regclass)
        )
        OR tenant_id = NULLIF(pg_catalog.current_setting('tect.tenant_id', true), '')::uuid
    );

CREATE POLICY source_worktrees_tenant_scope ON source_worktrees
    USING (
        CURRENT_USER = pg_catalog.pg_get_userbyid(
            (SELECT relowner FROM pg_catalog.pg_class WHERE oid='public.source_worktrees'::regclass)
        )
        OR tenant_id = NULLIF(pg_catalog.current_setting('tect.tenant_id', true), '')::uuid
    )
    WITH CHECK (
        CURRENT_USER = pg_catalog.pg_get_userbyid(
            (SELECT relowner FROM pg_catalog.pg_class WHERE oid='public.source_worktrees'::regclass)
        )
        OR tenant_id = NULLIF(pg_catalog.current_setting('tect.tenant_id', true), '')::uuid
    );

CREATE POLICY session_worktrees_tenant_scope ON session_worktrees
    USING (
        CURRENT_USER = pg_catalog.pg_get_userbyid(
            (SELECT relowner FROM pg_catalog.pg_class WHERE oid='public.session_worktrees'::regclass)
        )
        OR tenant_id = NULLIF(pg_catalog.current_setting('tect.tenant_id', true), '')::uuid
    )
    WITH CHECK (
        CURRENT_USER = pg_catalog.pg_get_userbyid(
            (SELECT relowner FROM pg_catalog.pg_class WHERE oid='public.session_worktrees'::regclass)
        )
        OR tenant_id = NULLIF(pg_catalog.current_setting('tect.tenant_id', true), '')::uuid
    );

REVOKE ALL PRIVILEGES ON TABLE source_repositories, source_worktrees, session_worktrees FROM PUBLIC;
