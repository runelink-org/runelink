-- Deleted home users, so names can't be reused while old access tokens
-- issued to the deleted user are still valid.
CREATE TABLE deleted_users (
    name TEXT NOT NULL,
    host TEXT NOT NULL,
    deleted_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (name, host)
);
