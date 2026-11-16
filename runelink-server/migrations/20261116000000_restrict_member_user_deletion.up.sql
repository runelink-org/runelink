-- Memberships of local servers are removed through their server actors, so
-- deleting a user who still has any is refused rather than cascading.
ALTER TABLE server_users
    DROP CONSTRAINT server_users_user_fkey,
    ADD CONSTRAINT server_users_user_fkey
        FOREIGN KEY (user_name, user_host)
        REFERENCES users(name, host)
        ON DELETE RESTRICT;
