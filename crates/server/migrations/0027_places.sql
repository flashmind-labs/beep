-- Places a user saved by name, such as Home or Gym. `inside` is whether the
-- phone's last usable fix was there, so arriving and leaving fire once each.
CREATE TABLE places (
    user_id TEXT NOT NULL,
    id TEXT NOT NULL,
    name TEXT NOT NULL,
    latitude DOUBLE PRECISION NOT NULL,
    longitude DOUBLE PRECISION NOT NULL,
    radius INTEGER NOT NULL,
    address TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    inside BOOLEAN NOT NULL DEFAULT FALSE,
    last_here_at TIMESTAMPTZ,
    PRIMARY KEY (user_id, id)
);
CREATE UNIQUE INDEX places_name ON places (user_id, lower(name));

-- A routine whose schedule is `arrive:<place id>` or `leave:<place id>`
-- starts at that place instead of at a time, and goes when the place does.
ALTER TABLE crons ADD COLUMN place_id TEXT GENERATED ALWAYS AS (
    CASE WHEN schedule ~ '^(arrive|leave):' THEN split_part(schedule, ':', 2) END
) STORED;
ALTER TABLE crons ADD CONSTRAINT crons_place FOREIGN KEY (user_id, place_id)
    REFERENCES places (user_id, id) ON DELETE CASCADE;
CREATE INDEX crons_place ON crons (user_id, place_id) WHERE place_id IS NOT NULL;
