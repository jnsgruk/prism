-- Fix GitHub review URLs to link to the PR page anchored at the review,
-- instead of the bare /reviews/{id} endpoint.
-- e.g. https://github.com/org/repo/pull/42/reviews/123456
--   -> https://github.com/org/repo/pull/42#pullrequestreview-123456
UPDATE activity.contributions
SET url = regexp_replace(url, '/reviews/(\d+)$', '#pullrequestreview-\1')
WHERE contribution_type = 'pr_review'
  AND platform = 'github'
  AND url ~ '/reviews/\d+$';
