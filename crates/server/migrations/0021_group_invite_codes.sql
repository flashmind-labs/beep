-- Invites are now typed codes of ten characters from the alphabet in
-- `store::groups`. Older long tokens get a fresh code; their links stop
-- working, as after a reset.
UPDATE groups SET invite = (
    SELECT string_agg(substr('23456789ABCDEFGHJKMNPQRSTWXYZ', 1 + floor(random() * 29)::int, 1), '')
    FROM generate_series(1, 10)
    -- Ties the subquery to the row, so each group draws its own code.
    WHERE groups.id IS NOT NULL
)
WHERE length(invite) <> 10;
