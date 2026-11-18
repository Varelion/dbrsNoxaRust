-- Where each Dragon Ball set can be searched for, and the roll a search needs.

-- A category covers every channel in it, including ones added later; a channel covers itself.
CREATE TABLE ball_areas (
    set_key    TEXT NOT NULL,
    channel_id INTEGER NOT NULL,
    kind       TEXT NOT NULL CHECK (kind IN ('category', 'channel')),
    PRIMARY KEY (set_key, channel_id)
) STRICT;

-- The 1d100 a search must reach. No row: the set's default (domain/dragon_balls.rs).
CREATE TABLE ball_rules (
    set_key TEXT PRIMARY KEY,
    need    INTEGER NOT NULL CHECK (need BETWEEN 1 AND 100)
) STRICT;
