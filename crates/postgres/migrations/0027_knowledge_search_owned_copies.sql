-- DK-3 search projections and durable jobs participate in owned-copy suppression.
ALTER TABLE knowledge_owned_copies
  DROP CONSTRAINT knowledge_owned_copies_relation_check;
ALTER TABLE knowledge_owned_copies
  ADD CONSTRAINT knowledge_owned_copies_relation_check CHECK (relation_name IN (
    'knowledge_changes','knowledge_command_receipts','knowledge_lifecycle_changes',
    'knowledge_change_runs','knowledge_change_operations','knowledge_change_outputs',
    'knowledge_change_attempts','knowledge_change_inputs','knowledge_lifecycle_command_receipts',
    'pipeline_knowledge_manifests','slice_pipeline_runs','slice_pipeline_phase_attempts',
    'slice_pipeline_phase_outputs','slice_pipeline_inputs','slice_pipeline_receipts',
    'slice_results','slice_planning_inputs','slice_planning_snapshots',
    'slice_candidate_drafts','slice_candidate_reviews','native_planning_receipts',
    'knowledge_search_resources','knowledge_search_embedding_jobs','knowledge_search_vectors'
  ));
