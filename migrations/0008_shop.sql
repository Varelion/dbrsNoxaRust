-- The shop: emoji items players buy with Spirit Points; staff keep the shelf.
CREATE TABLE shop_items (
    id          INTEGER PRIMARY KEY,
    emoji       TEXT NOT NULL,
    name        TEXT NOT NULL COLLATE NOCASE UNIQUE,
    description TEXT NOT NULL DEFAULT '',
    price       INTEGER NOT NULL CHECK (price >= 0),
    listed      INTEGER NOT NULL DEFAULT 1    -- 0: hidden from players; owners keep theirs
) STRICT;

-- What each player owns from the shop. Removing an item from the shop removes it here too.
CREATE TABLE owned_items (
    player_id INTEGER NOT NULL,
    item_id   INTEGER NOT NULL REFERENCES shop_items (id) ON DELETE CASCADE,
    count     INTEGER NOT NULL CHECK (count > 0),
    PRIMARY KEY (player_id, item_id)
) STRICT;

-- A mock shelf to start with; staff change it on the admin pages.
INSERT INTO shop_items (emoji, name, description, price) VALUES
    ('🎁', 'Starter Capsule', 'A welcome gift. Free, one each.', 0),
    ('🍖', 'Meat on the Bone', 'Fills a Saiyan for almost an hour.', 5),
    ('💊', 'Hoi-Poi Capsule', 'Empty. Holds anything you can lift.', 10),
    ('🧸', 'Dragon Plushie', 'Soft, green, and grants no wishes.', 15),
    ('🥋', 'Weighted Gi', 'Every step is training.', 20),
    ('🫘', 'Senzu Bean', 'Heals any wound at once. One use.', 25),
    ('🛵', 'Capsule Scooter', 'Pops out of a capsule, ready to ride.', 40),
    ('🏠', 'Capsule House', 'A home in your pocket.', 60),
    ('☁️', 'Flying Nimbus', 'Only the pure of heart can ride it.', 80);
