-- The user's fork an import cloned as `origin`, `login/repo` on the
-- project's host; NULL when it cloned the repository itself.
ALTER TABLE project_imports ADD COLUMN fork TEXT;
