-- A place routine that runs only the next time now says so in its schedule,
-- as `arrive:<place id>:once`, instead of only in run_once.
UPDATE crons
SET schedule = split_part(schedule, ':', 1) || ':' || split_part(schedule, ':', 2) || ':once'
WHERE run_once AND schedule ~ '^(arrive|leave):';
