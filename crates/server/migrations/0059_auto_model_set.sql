-- Whether the user ever set Auto. Auto is on until they turn it off, so an
-- unset `auto_model` (still its old default, false) reads as on.
ALTER TABLE users ADD COLUMN auto_model_set BOOLEAN NOT NULL DEFAULT false;
UPDATE users SET auto_model_set = true WHERE auto_model;
