-- The free item is a joke now: everyone's first friend.
UPDATE shop_items SET emoji = '🧑‍💻', name = 'Tom (MySpace)', description = 'Your first friend. Free, one each.'
WHERE name = 'Starter Capsule';
