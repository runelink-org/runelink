-- Cleared text cannot be recovered by rolling back the schema.
ALTER TABLE messages DROP CONSTRAINT messages_content_state;
ALTER TABLE messages DISABLE TRIGGER messages_set_updated_at;
UPDATE messages SET body = '' WHERE body IS NULL;
ALTER TABLE messages ENABLE TRIGGER messages_set_updated_at;
ALTER TABLE messages
    ALTER COLUMN body SET NOT NULL,
    DROP COLUMN deleted_at,
    DROP COLUMN reply_to;
