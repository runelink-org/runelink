-- Collapse server channels back into flat channels. Categories can't be
-- represented, so their children move to the root and they are deleted.

DROP TRIGGER server_channels_set_updated_at ON server_channels;
DROP TRIGGER server_channels_delete_text_channel ON server_channels;
DROP FUNCTION delete_owned_text_channel();
ALTER TABLE messages DISABLE TRIGGER messages_set_updated_at;

UPDATE server_channels SET parent_channel_id = NULL;
DELETE FROM server_channels WHERE kind = 'category';

-- MESSAGES --

ALTER TABLE messages DROP CONSTRAINT messages_text_channel_id_fkey;

UPDATE messages m
SET text_channel_id = sc.id
FROM server_channels sc
WHERE m.text_channel_id = sc.text_channel_id;

ALTER TABLE messages RENAME COLUMN text_channel_id TO channel_id;
ALTER INDEX idx_messages_text_channel_id_created_at
    RENAME TO idx_messages_channel_id_created_at;

-- CHANNELS --

DROP INDEX idx_server_channels_siblings;

ALTER TABLE server_channels
    DROP CONSTRAINT server_channels_parent_fkey,
    DROP CONSTRAINT server_channels_server_id_id_key,
    DROP CONSTRAINT server_channels_text_channel_id_fkey,
    DROP COLUMN kind,
    DROP COLUMN text_channel_id,
    DROP COLUMN parent_channel_id,
    DROP COLUMN position;

DROP TABLE text_channels;

ALTER TABLE server_channels
    RENAME CONSTRAINT server_channels_server_id_fkey TO channels_server_id_fkey;
ALTER TABLE server_channels
    RENAME CONSTRAINT server_channels_pkey TO channels_pkey;
ALTER TABLE server_channels RENAME TO channels;

CREATE INDEX idx_channels_server_id
    ON channels (server_id);

ALTER TABLE messages
    ADD CONSTRAINT messages_channel_id_fkey
        FOREIGN KEY (channel_id) REFERENCES channels (id)
        ON DELETE CASCADE;

CREATE TRIGGER channels_set_updated_at
    BEFORE UPDATE ON channels
    FOR EACH ROW
    EXECUTE FUNCTION set_updated_at();

ALTER TABLE messages ENABLE TRIGGER messages_set_updated_at;
