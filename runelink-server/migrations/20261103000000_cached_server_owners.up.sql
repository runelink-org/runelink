-- Cached remote servers are compared by canonical host (lowercase, without
-- the default port), and can never claim the ID of a local server.
DELETE FROM cached_remote_servers c
USING servers s
WHERE c.id = s.id;

UPDATE cached_remote_servers
SET host = lower(regexp_replace(host, ':7000$', ''))
WHERE host <> lower(regexp_replace(host, ':7000$', ''));
