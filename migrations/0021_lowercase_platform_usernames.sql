-- Normalise platform usernames to lowercase for case-insensitive matching.
-- All affected platforms (GitHub, Discourse, Launchpad, Mattermost, Jira/email)
-- treat usernames as case-insensitive.

-- Step 1: Deduplicate identities that would collide on (platform, LOWER(platform_username)).
DO $$
DECLARE
    dup RECORD;
    winner_id UUID;
    loser_ids UUID[];
BEGIN
    FOR dup IN
        SELECT platform, LOWER(platform_username) AS norm,
               array_agg(id ORDER BY id) AS ids,
               array_agg(person_id ORDER BY id) AS person_ids
        FROM org.platform_identities
        GROUP BY platform, LOWER(platform_username)
        HAVING COUNT(*) > 1
    LOOP
        -- Keep the first (oldest) identity as winner.
        winner_id := dup.ids[1];
        loser_ids := dup.ids[2:];

        -- Reassign contributions from loser person_ids to winner's person_id.
        UPDATE activity.contributions
        SET person_id = (SELECT person_id FROM org.platform_identities WHERE id = winner_id)
        WHERE person_id = ANY(
            SELECT person_id FROM org.platform_identities WHERE id = ANY(loser_ids)
        );

        -- Delete loser identities.
        DELETE FROM org.platform_identities WHERE id = ANY(loser_ids);
    END LOOP;
END $$;

-- Step 2: Lowercase all platform usernames.

-- Identities
UPDATE org.platform_identities
SET platform_username = LOWER(platform_username)
WHERE platform_username != LOWER(platform_username);

-- GitHub team members (separate table, separate PK)
UPDATE org.github_team_members
SET github_username = LOWER(github_username)
WHERE github_username != LOWER(github_username);

-- Discourse metadata in contributions (used by backfill_discourse_person_ids)
UPDATE activity.contributions
SET metadata = jsonb_set(metadata, '{username}', to_jsonb(LOWER(metadata->>'username')))
WHERE metadata->>'username' IS NOT NULL
  AND metadata->>'username' != LOWER(metadata->>'username');

-- Step 3: Re-link orphaned Discourse contributions that now match after normalisation.
UPDATE activity.contributions c
SET person_id = pi.person_id
FROM org.platform_identities pi
WHERE c.person_id IS NULL
  AND pi.platform = c.platform
  AND pi.platform_username = LOWER(c.metadata->>'username')
  AND c.platform LIKE 'discourse-%';
