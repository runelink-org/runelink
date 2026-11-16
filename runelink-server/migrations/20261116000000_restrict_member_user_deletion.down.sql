ALTER TABLE server_users
    DROP CONSTRAINT server_users_user_fkey,
    ADD CONSTRAINT server_users_user_fkey
        FOREIGN KEY (user_name, user_host)
        REFERENCES users(name, host)
        ON DELETE CASCADE;
