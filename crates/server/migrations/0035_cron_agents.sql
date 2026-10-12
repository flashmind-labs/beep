-- The agent of the turn that set up a routine, so its runs answer as that
-- agent. NULL for routines made with agents off, which run on General.
ALTER TABLE crons ADD COLUMN agent TEXT;
