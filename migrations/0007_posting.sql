-- Prefixes are unique per player (the cast counts as one), not server-wide: two players may both use "k".
DROP INDEX characters_prefix;
CREATE UNIQUE INDEX characters_prefix ON characters (player_id, lower(prefix)) WHERE prefix IS NOT NULL;

-- posting: 1 posts by prefix, 0 not. posts: how many roleplay posts the character has made.
ALTER TABLE characters ADD COLUMN posting INTEGER NOT NULL DEFAULT 1;
ALTER TABLE characters ADD COLUMN posts INTEGER NOT NULL DEFAULT 0;

-- Expressions: named pictures a character posts with, "nova:happy text". color: a hex bar, or none.
CREATE TABLE expressions (
    character_id INTEGER NOT NULL REFERENCES characters (id),
    name         TEXT NOT NULL,
    url          TEXT NOT NULL,
    color        INTEGER,
    PRIMARY KEY (character_id, name)
) STRICT;

-- Every part of a post made as a character, so its author can edit or delete it.
-- channel_id: where it is (a thread's own id); hook_channel: the webhook's (a thread's parent).
CREATE TABLE proxied (
    message_id   INTEGER PRIMARY KEY,
    channel_id   INTEGER NOT NULL,
    hook_channel INTEGER NOT NULL,
    thread_id    INTEGER,
    webhook_id   INTEGER NOT NULL,
    author_id    INTEGER NOT NULL,
    character_id INTEGER NOT NULL,
    group_id     INTEGER NOT NULL,
    part         INTEGER NOT NULL,
    at           INTEGER NOT NULL
) STRICT;
CREATE INDEX proxied_group ON proxied (group_id);

-- The interactive tutorial: the step each person is on, and the thread it runs in.
CREATE TABLE tutorials (
    user_id    INTEGER PRIMARY KEY,
    track      TEXT NOT NULL,
    step       TEXT NOT NULL,
    thread_id  INTEGER,
    started_at INTEGER NOT NULL
) STRICT;
