-- Runtime row locks require UPDATE(id), but policy values remain immutable.
-- Keep the owner-side immutability trigger active in every replication mode.
ALTER TABLE advisory_budget_policies
    ENABLE ALWAYS TRIGGER advisory_budget_policy_guard_trigger;
