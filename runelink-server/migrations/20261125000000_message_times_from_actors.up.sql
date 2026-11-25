-- Channel actors choose every message time, including updated_at, so that
-- they know what is stored without reading it back. Changes the database makes
-- itself, like clearing the author of a deleted user, no longer move
-- updated_at; no edit can depend on them, since only a message's author can
-- edit it.
DROP TRIGGER messages_set_updated_at ON messages;

-- For the latest time in a channel, where its actor's clock starts.
CREATE INDEX idx_messages_text_channel_id_updated_at
    ON messages (text_channel_id, updated_at);
