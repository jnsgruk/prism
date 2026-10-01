-- Preserve existing imported provenance; administrators opt in to manual choices.
ALTER TABLE org.people ADD COLUMN membership_management TEXT NOT NULL DEFAULT 'imported'
    CHECK (membership_management IN ('imported', 'manual'));
ALTER TABLE org.platform_identities ADD COLUMN management TEXT NOT NULL DEFAULT 'imported'
    CHECK (management IN ('imported', 'manual'));
-- Fail with repair instructions rather than selecting an owner or deleting data.
DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM org.platform_identities GROUP BY platform, lower(platform_username) HAVING count(*) > 1) THEN
        RAISE EXCEPTION 'Duplicate case-insensitive usernames; inspect org.platform_identities grouped by platform and lower(platform_username), repair ownership, and retry';
    END IF;
    IF EXISTS (SELECT 1 FROM org.platform_identities WHERE platform = 'jira' AND platform_user_id IS NOT NULL GROUP BY platform_user_id HAVING count(*) > 1) THEN
        RAISE EXCEPTION 'Duplicate Jira account IDs; inspect org.platform_identities grouped by platform_user_id for jira, repair ownership, and retry';
    END IF;
END $$;
UPDATE org.platform_identities SET platform_username = lower(platform_username) WHERE platform_username <> lower(platform_username);
CREATE UNIQUE INDEX idx_identity_username_case ON org.platform_identities (platform, lower(platform_username));
CREATE UNIQUE INDEX idx_identity_jira_account ON org.platform_identities (platform_user_id) WHERE platform = 'jira' AND platform_user_id IS NOT NULL;
CREATE FUNCTION org.prevent_identity_owner_change() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF NEW.person_id IS DISTINCT FROM OLD.person_id THEN
        RAISE EXCEPTION 'Identity ownership cannot be changed' USING ERRCODE = '23505';
    END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER identity_owner_immutable BEFORE UPDATE ON org.platform_identities
    FOR EACH ROW EXECUTE FUNCTION org.prevent_identity_owner_change();
