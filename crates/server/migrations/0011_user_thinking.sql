-- The reasoning level an admin picked for their own turns; NULL uses the config's.
ALTER TABLE users ADD COLUMN thinking TEXT;
