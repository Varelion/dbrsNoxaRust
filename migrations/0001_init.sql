-- noxaDBRS's first schema. Discord ids are INTEGER: snowflakes fit in a signed 64-bit integer.

CREATE TABLE settings (
    name  TEXT PRIMARY KEY,
    value TEXT NOT NULL
) STRICT;

-- A player's characters, one per slot.
CREATE TABLE characters (
    id        INTEGER PRIMARY KEY,
    player_id INTEGER NOT NULL,
    slot      INTEGER NOT NULL,
    name      TEXT NOT NULL,
    added_by  INTEGER NOT NULL,
    added_at  INTEGER NOT NULL,
    UNIQUE (player_id, slot)
) STRICT;

-- The character each player plays now (/set): posts and Log feat go to it.
CREATE TABLE active_characters (
    player_id    INTEGER PRIMARY KEY,
    character_id INTEGER NOT NULL REFERENCES characters (id)
) STRICT;

-- Spirit Points, and every change to them: never edited.
CREATE TABLE wallets (
    player_id INTEGER PRIMARY KEY,
    sp        INTEGER NOT NULL DEFAULT 0
) STRICT;

CREATE TABLE ledger (
    id        INTEGER PRIMARY KEY,
    player_id INTEGER NOT NULL,
    delta     INTEGER NOT NULL,
    balance   INTEGER NOT NULL,
    kind      TEXT NOT NULL,      -- give | take | set | grant
    memo      TEXT NOT NULL,
    staff_id  INTEGER NOT NULL,
    at        INTEGER NOT NULL
) STRICT;
CREATE INDEX ledger_player ON ledger (player_id, id);

-- Silent feats: archived, never deleted.
CREATE TABLE feats (
    id             INTEGER PRIMARY KEY,
    character_id   INTEGER NOT NULL REFERENCES characters (id),
    kind           TEXT NOT NULL,
    counts         TEXT NOT NULL,
    note           TEXT NOT NULL,
    added_by       INTEGER NOT NULL,
    added_at       INTEGER NOT NULL,
    edited_by      INTEGER,
    edited_at      INTEGER,
    archived_by    INTEGER,          -- NULL: active
    archived_at    INTEGER,
    archive_reason TEXT
) STRICT;
CREATE INDEX feats_character ON feats (character_id, id);

CREATE TABLE feat_links (
    id         INTEGER PRIMARY KEY,
    feat_id    INTEGER NOT NULL REFERENCES feats (id),
    channel_id INTEGER NOT NULL,
    message_id INTEGER NOT NULL,
    added_by   INTEGER NOT NULL,
    added_at   INTEGER NOT NULL,
    UNIQUE (feat_id, message_id)
) STRICT;
