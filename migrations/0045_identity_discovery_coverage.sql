-- Supplementary user discovery is independent from source-wide watermarks.
CREATE TABLE activity.identity_discovery_coverage (
    source_id UUID NOT NULL REFERENCES config.source_configs(id) ON DELETE CASCADE,
    identity_id UUID NOT NULL REFERENCES org.platform_identities(id) ON DELETE CASCADE,
    identity_version TEXT NOT NULL,
    covered_through TIMESTAMPTZ NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (source_id, identity_id)
);

-- Editing one saved account resets only coverage attached to that account.
CREATE FUNCTION activity.reset_identity_discovery_coverage() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF (NEW.platform, NEW.platform_username, NEW.platform_user_id)
       IS DISTINCT FROM (OLD.platform, OLD.platform_username, OLD.platform_user_id) THEN
        DELETE FROM activity.identity_discovery_coverage WHERE identity_id = NEW.id;
    END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER identity_discovery_reset AFTER UPDATE ON org.platform_identities
    FOR EACH ROW EXECUTE FUNCTION activity.reset_identity_discovery_coverage();
