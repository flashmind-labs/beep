-- Whether Jev picks an admin's model and thinking per prompt.
ALTER TABLE users ADD COLUMN auto_model BOOLEAN NOT NULL DEFAULT false;
-- The thinking Jev picked for a turn; NULL when it did not pick.
ALTER TABLE turn_models ADD COLUMN thinking TEXT;
