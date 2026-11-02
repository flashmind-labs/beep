-- Interactive terminals: a node's `terminal/1` pseudo-terminal rather than
-- a `process/1` background task. `task_id` is then the node's terminal
-- number.
ALTER TABLE terminals ADD COLUMN pty BOOLEAN NOT NULL DEFAULT FALSE;
