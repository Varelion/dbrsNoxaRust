-- Profiles, inventories and Dragon Balls.

-- What a profile shows. NULL picture_url: the owner's Discord profile icon. NULL tier: not set yet.
ALTER TABLE characters ADD COLUMN race TEXT;
ALTER TABLE characters ADD COLUMN tier INTEGER;
ALTER TABLE characters ADD COLUMN title TEXT;
ALTER TABLE characters ADD COLUMN bio TEXT;
ALTER TABLE characters ADD COLUMN picture_url TEXT;

-- What a character carries: an item key from the catalog (domain/items.rs) and how many.
CREATE TABLE inventory (
    character_id INTEGER NOT NULL REFERENCES characters (id),
    item         TEXT NOT NULL,
    quantity     INTEGER NOT NULL CHECK (quantity > 0),
    PRIMARY KEY (character_id, item)
) STRICT;

-- Every ball of every set, and who holds it. NULL holder: hidden in the world (channel_id, staff only).
CREATE TABLE dragon_balls (
    set_key   TEXT NOT NULL,                 -- domain/dragon_balls.rs BallSet
    star      INTEGER NOT NULL CHECK (star BETWEEN 1 AND 7),
    holder_id INTEGER REFERENCES characters (id),
    channel_id INTEGER,
    PRIMARY KEY (set_key, star)
) STRICT;
CREATE INDEX dragon_balls_holder ON dragon_balls (holder_id);
