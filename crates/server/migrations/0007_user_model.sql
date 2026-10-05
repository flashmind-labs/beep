-- The model an admin picked for their own turns. NULL means the server default.
ALTER TABLE users ADD COLUMN model TEXT;
