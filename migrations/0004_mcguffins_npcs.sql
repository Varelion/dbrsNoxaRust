-- McGuffins: plot items staff hand out by hand. Retired, never deleted.
CREATE TABLE mcguffins (
    id          INTEGER PRIMARY KEY,
    name        TEXT NOT NULL,
    emoji       TEXT NOT NULL,
    description TEXT NOT NULL,
    holding     TEXT NOT NULL CHECK (holding IN ('one', 'many')),  -- one holder at a time, or any number
    created_by  INTEGER NOT NULL,
    created_at  INTEGER NOT NULL,
    retired_by  INTEGER,                                            -- NULL: in play
    retired_at  INTEGER
) STRICT;
CREATE UNIQUE INDEX mcguffins_name ON mcguffins (lower(name));

CREATE TABLE mcguffin_holders (
    mcguffin_id  INTEGER NOT NULL REFERENCES mcguffins (id),
    character_id INTEGER NOT NULL REFERENCES characters (id),
    given_by     INTEGER NOT NULL,
    given_at     INTEGER NOT NULL,
    PRIMARY KEY (mcguffin_id, character_id)
) STRICT;

-- NPCs are the cast account's characters (player_id 0). prefix: for posting as them; notes: staff only.
ALTER TABLE characters ADD COLUMN prefix TEXT;
ALTER TABLE characters ADD COLUMN notes TEXT;
CREATE UNIQUE INDEX characters_prefix ON characters (lower(prefix)) WHERE prefix IS NOT NULL;
