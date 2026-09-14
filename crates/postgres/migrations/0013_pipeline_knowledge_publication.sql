-- DK-2 producer outputs retain an exact, backend-resolved publication reference.
ALTER TABLE slice_pipeline_phase_outputs
  ADD COLUMN knowledge_publication jsonb,
  ADD CONSTRAINT slice_pipeline_outputs_knowledge_publication_shape CHECK (
    knowledge_publication IS NULL
    OR pg_catalog.jsonb_typeof(knowledge_publication)='object'
  );
