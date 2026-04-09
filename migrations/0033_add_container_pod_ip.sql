-- Add pod IP column so the frontend can display pod details.
ALTER TABLE reasoning.conversations ADD COLUMN IF NOT EXISTS container_pod_ip TEXT;

-- Reset stale container_status values. Before this migration the reaper
-- never updated container_status, so every conversation that ever had a
-- pod still shows "active". Reset them all to "pending" (the default) —
-- the next pod creation will set "active" correctly.
UPDATE reasoning.conversations
SET container_status = 'pending'
WHERE container_status = 'active';
