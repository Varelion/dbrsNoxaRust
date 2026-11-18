-- Worlds: the categories staff mark as worlds. Their text channels are the roleplay channels.
CREATE TABLE worlds (
    category_id INTEGER PRIMARY KEY
) STRICT;

-- Where a McGuffin lies while nobody carries it: in a world (a category), in one channel, or in a
-- channel rolled from every roleplay channel. Both NULL: nowhere.
ALTER TABLE mcguffins ADD COLUMN place_kind TEXT CHECK (place_kind IN ('world', 'channel', 'random'));
ALTER TABLE mcguffins ADD COLUMN place_id INTEGER;
