-- Only one server per ID can be kept, and local servers keep theirs.
DELETE FROM cached_remote_servers c
USING servers s
WHERE c.id = s.id;

DELETE FROM cached_remote_servers c
USING cached_remote_servers other
WHERE c.id = other.id AND c.host > other.host;

DROP INDEX idx_user_remote_memberships_remote_server;

ALTER TABLE user_remote_server_memberships
    DROP CONSTRAINT user_remote_server_memberships_remote_server_fkey,
    DROP CONSTRAINT user_remote_server_memberships_pkey;

ALTER TABLE cached_remote_servers
    DROP CONSTRAINT cached_remote_servers_pkey,
    ADD PRIMARY KEY (id);

ALTER TABLE user_remote_server_memberships
    DROP COLUMN remote_server_host,
    ADD PRIMARY KEY (user_name, user_host, remote_server_id),
    ADD CONSTRAINT user_remote_server_memberships_remote_server_id_fkey
        FOREIGN KEY (remote_server_id)
        REFERENCES cached_remote_servers (id)
        ON DELETE CASCADE;

CREATE INDEX idx_user_remote_memberships_remote_server_id
    ON user_remote_server_memberships (remote_server_id);
