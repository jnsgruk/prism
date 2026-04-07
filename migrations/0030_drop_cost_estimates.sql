-- Drop cost estimation columns — actual cost tracking is done externally via
-- the Google Cloud billing dashboard.  We keep token counts and request counts.

ALTER TABLE reasoning.api_usage DROP COLUMN IF EXISTS estimated_cost_usd;
ALTER TABLE reasoning.conversations DROP COLUMN IF EXISTS total_estimated_cost_usd;
