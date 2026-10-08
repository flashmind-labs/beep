-- Invites are now typed codes, read uppercase, so older tokens are stored
-- the same way and still match.
UPDATE groups SET invite = upper(invite);
