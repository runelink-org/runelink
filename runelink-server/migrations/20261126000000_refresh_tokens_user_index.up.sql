-- For deleting a user's expired and revoked refresh tokens when issuing one.
CREATE INDEX idx_refresh_tokens_user
    ON refresh_tokens (user_name, user_host);
