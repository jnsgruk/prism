-- Jira display names are labels. Only opaque account IDs identify Cloud accounts.
ALTER TABLE org.platform_identities
    DROP CONSTRAINT platform_identities_platform_platform_username_key;
DROP INDEX org.idx_identity_username_case;

-- Legacy Jira rows without account IDs still resolve by username.
CREATE UNIQUE INDEX idx_identity_username ON org.platform_identities (platform, platform_username)
    WHERE platform <> 'jira' OR platform_user_id IS NULL;
CREATE UNIQUE INDEX idx_identity_username_case ON org.platform_identities (platform, lower(platform_username))
    WHERE platform <> 'jira' OR platform_user_id IS NULL;

-- idx_identity_jira_account and identity_owner_immutable continue to enforce
-- account-ID uniqueness and immutable ownership, including concurrent writes.
