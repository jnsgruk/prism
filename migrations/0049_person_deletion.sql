-- Retain source activity, teams and login accounts when an inactive person is deleted.
-- Memberships and identities remain restrictive and are removed by OrgRepo.
ALTER TABLE activity.contributions
    DROP CONSTRAINT contributions_person_id_fkey,
    ADD CONSTRAINT contributions_person_id_fkey
        FOREIGN KEY (person_id) REFERENCES org.people(id) ON DELETE SET NULL;

ALTER TABLE org.teams
    DROP CONSTRAINT teams_lead_id_fkey,
    ADD CONSTRAINT teams_lead_id_fkey
        FOREIGN KEY (lead_id) REFERENCES org.people(id) ON DELETE SET NULL;

ALTER TABLE auth.users
    DROP CONSTRAINT users_person_id_fkey,
    ADD CONSTRAINT users_person_id_fkey
        FOREIGN KEY (person_id) REFERENCES org.people(id) ON DELETE SET NULL;
