-- Match AI results to the queued source input, separately from the prompt hash.
-- Unknown legacy provenance stays NULL and cannot satisfy new queued work.
ALTER TABLE reasoning.enrichments ADD COLUMN source_content_hash TEXT;
