-- A picture uploaded from the admin pages is an attachment in Discord's pictures channel. Its link
-- expires, so the message is kept too: a fresh link is fetched from it when the old one runs out.
ALTER TABLE characters ADD COLUMN picture_message INTEGER;
