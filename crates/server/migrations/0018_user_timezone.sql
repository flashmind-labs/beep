-- The timezone the user's phone is set to, sent on connect so it is known
-- without a location fix. NULL means UTC.
ALTER TABLE users ADD COLUMN timezone TEXT;
UPDATE users SET timezone = locations.timezone FROM locations WHERE locations.user_id = users.id;
