-- Split server-facing channels from the underlying message containers.
-- Existing channels become root-level `text` server channels, each owning a
-- new text channel that receives its messages.

-- Preserve existing updated_at values while backfilling
DROP TRIGGER channels_set_updated_at ON channels;
ALTER TABLE messages DISABLE TRIGGER messages_set_updated_at;

-- TEXT CHANNELS --

CREATE TABLE text_channels (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid()
);

-- SERVER CHANNELS --

ALTER TABLE channels RENAME TO server_channels;
ALTER TABLE server_channels
    RENAME CONSTRAINT channels_pkey TO server_channels_pkey;
ALTER TABLE server_channels
    RENAME CONSTRAINT channels_server_id_fkey TO server_channels_server_id_fkey;
DROP INDEX idx_channels_server_id;

ALTER TABLE server_channels
    ADD COLUMN kind TEXT NOT NULL DEFAULT 'text',
    ADD COLUMN text_channel_id UUID,
    ADD COLUMN parent_channel_id UUID,
    ADD COLUMN position INTEGER;

UPDATE server_channels SET text_channel_id = gen_random_uuid();
INSERT INTO text_channels (id) SELECT text_channel_id FROM server_channels;

UPDATE server_channels sc
SET position = ordered.position
FROM (
    SELECT
        id,
        (ROW_NUMBER() OVER (
            PARTITION BY server_id ORDER BY created_at, id
        ) - 1)::INTEGER AS position
    FROM server_channels
) ordered
WHERE sc.id = ordered.id;

ALTER TABLE server_channels
    ALTER COLUMN kind DROP DEFAULT,
    ALTER COLUMN position SET NOT NULL,
    ADD CONSTRAINT server_channels_kind_check
        CHECK (kind IN ('text', 'category')),
    -- Text channels own exactly one text channel; categories own none
    ADD CONSTRAINT server_channels_text_channel_check
        CHECK ((kind = 'text') = (text_channel_id IS NOT NULL)),
    ADD CONSTRAINT server_channels_text_channel_id_key
        UNIQUE (text_channel_id),
    ADD CONSTRAINT server_channels_text_channel_id_fkey
        FOREIGN KEY (text_channel_id) REFERENCES text_channels (id),
    ADD CONSTRAINT server_channels_server_id_id_key UNIQUE (server_id, id),
    -- Parents must be in the same server. Parent kind and cycles are
    -- validated by the application. Non-empty parents can't be deleted.
    ADD CONSTRAINT server_channels_parent_fkey
        FOREIGN KEY (server_id, parent_channel_id)
        REFERENCES server_channels (server_id, id);

-- MESSAGES --

ALTER TABLE messages DROP CONSTRAINT messages_channel_id_fkey;
ALTER TABLE messages RENAME COLUMN channel_id TO text_channel_id;
ALTER INDEX idx_messages_channel_id_created_at
    RENAME TO idx_messages_text_channel_id_created_at;

UPDATE messages m
SET text_channel_id = sc.text_channel_id
FROM server_channels sc
WHERE m.text_channel_id = sc.id;

ALTER TABLE messages
    ADD CONSTRAINT messages_text_channel_id_fkey
        FOREIGN KEY (text_channel_id) REFERENCES text_channels (id)
        ON DELETE CASCADE;

-- INDEXES --

CREATE INDEX idx_server_channels_siblings
    ON server_channels (server_id, parent_channel_id, position, id);

-- OWNERSHIP --

-- Deleting a server channel, including through server deletion, also
-- deletes the text channel it owns and therefore its messages.
CREATE OR REPLACE FUNCTION delete_owned_text_channel()
    RETURNS TRIGGER AS $$
BEGIN
    DELETE FROM text_channels WHERE id = OLD.text_channel_id;
    RETURN OLD;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER server_channels_delete_text_channel
    AFTER DELETE ON server_channels
    FOR EACH ROW
    WHEN (OLD.text_channel_id IS NOT NULL)
    EXECUTE FUNCTION delete_owned_text_channel();

-- UPDATED_AT MANAGEMENT --

CREATE TRIGGER server_channels_set_updated_at
    BEFORE UPDATE ON server_channels
    FOR EACH ROW
    EXECUTE FUNCTION set_updated_at();

ALTER TABLE messages ENABLE TRIGGER messages_set_updated_at;
