-- Reply identity is independent of channel placement. The API currently
-- restricts replies to the same text channel; the schema permits cross-channel replies.
ALTER TABLE messages
    ALTER COLUMN body DROP NOT NULL,
    ADD COLUMN deleted_at TIMESTAMPTZ,
    ADD COLUMN reply_to UUID REFERENCES messages (id) ON DELETE SET NULL,
    ADD CONSTRAINT messages_content_state CHECK (
        (deleted_at IS NULL AND body IS NOT NULL)
        OR (deleted_at IS NOT NULL AND body IS NULL)
    );
CREATE INDEX idx_messages_reply_to ON messages (reply_to) WHERE reply_to IS NOT NULL;
