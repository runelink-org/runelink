-- Resource IDs are only unique within their owning host, so cached remote
-- servers are identified by their host and ID together.
ALTER TABLE user_remote_server_memberships
    ADD COLUMN remote_server_host TEXT;

UPDATE user_remote_server_memberships m
SET remote_server_host = s.host
FROM cached_remote_servers s
WHERE s.id = m.remote_server_id;

ALTER TABLE user_remote_server_memberships
    ALTER COLUMN remote_server_host SET NOT NULL,
    DROP CONSTRAINT user_remote_server_memberships_remote_server_id_fkey,
    DROP CONSTRAINT user_remote_server_memberships_pkey;

DROP INDEX idx_user_remote_memberships_remote_server_id;

ALTER TABLE cached_remote_servers
    DROP CONSTRAINT cached_remote_servers_pkey,
    ADD PRIMARY KEY (host, id);

ALTER TABLE user_remote_server_memberships
    ADD PRIMARY KEY (
        user_name, user_host, remote_server_host, remote_server_id
    ),
    ADD CONSTRAINT user_remote_server_memberships_remote_server_fkey
        FOREIGN KEY (remote_server_host, remote_server_id)
        REFERENCES cached_remote_servers (host, id)
        ON DELETE CASCADE;

CREATE INDEX idx_user_remote_memberships_remote_server
    ON user_remote_server_memberships (remote_server_host, remote_server_id);
