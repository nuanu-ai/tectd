-- A manifest is only a set of possible drafts. An active anti-bloat binding
-- requires the selected draft and its ordinary save receipt in the same lineage.
ALTER TABLE scope_anti_bloat_bindings
    ADD COLUMN selected_draft_revision bigint,
    ADD COLUMN selected_material_digest text,
    ADD COLUMN selected_alternative_id text,
    ADD COLUMN selected_caller_link_id uuid,
    ADD COLUMN selected_caller_request_id uuid,
    ADD CONSTRAINT scope_anti_bloat_selected_binding_complete CHECK (
        (selected_draft_revision IS NULL AND selected_material_digest IS NULL
         AND selected_alternative_id IS NULL AND selected_caller_link_id IS NULL
         AND selected_caller_request_id IS NULL)
        OR
        (selected_draft_revision IS NOT NULL
         AND selected_material_digest IS NOT NULL
         AND selected_alternative_id IS NOT NULL
         AND selected_draft_revision = candidate_set_revision
         AND selected_material_digest ~ '^[0-9a-f]{64}$'
         AND selected_alternative_id ~ '^[0-9a-f]{64}$'
         AND selected_caller_link_id IS NOT NULL
         AND selected_caller_request_id IS NOT NULL)
    ),
    ADD CONSTRAINT scope_anti_bloat_selected_draft_fk FOREIGN KEY
        (tenant_id,workspace_id,candidate_set_id,selected_draft_revision)
        REFERENCES scope_candidate_drafts
        (tenant_id,workspace_id,candidate_set_id,set_revision),
    ADD CONSTRAINT scope_anti_bloat_selected_caller_fk FOREIGN KEY
        (tenant_id,workspace_id,opportunity_id,candidate_set_id,
         selected_caller_link_id,selected_draft_revision)
        REFERENCES advisory_scope_caller_link
        (tenant_id,workspace_id,opportunity_id,candidate_set_id,
         link_id,caller_result_revision);
