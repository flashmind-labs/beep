-- Every imported project gets a project row, so its agent and board exist
-- before its chat is opened. Imports now make the row themselves; this
-- fills it in for the ones from before. One row per name in any case, and
-- only names that are owner/repo.
INSERT INTO projects (id, user_id, name)
SELECT gen_random_uuid(), i.user_id, i.project
FROM (
    SELECT DISTINCT ON (user_id, lower(project)) user_id, project
    FROM project_imports
    ORDER BY user_id, lower(project), imported_at
) i
WHERE i.project ~ '^[A-Za-z0-9._-]{1,100}/[A-Za-z0-9._-]{1,100}$'
  AND split_part(i.project, '/', 1) NOT IN ('.', '..')
  AND split_part(i.project, '/', 2) NOT IN ('.', '..')
  AND NOT EXISTS (
      SELECT 1 FROM projects p
      WHERE p.user_id = i.user_id AND lower(p.name) = lower(i.project)
  );
