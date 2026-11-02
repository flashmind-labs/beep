-- A review's reviewers read a detached worktree of the branch head, removed
-- once the review is done. NULL for reviews from before, and once removed.
ALTER TABLE branch_reviews ADD COLUMN worktree TEXT;
