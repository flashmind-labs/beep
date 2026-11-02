-- Several GitHub accounts per user. A GitHub row keeps its login in
-- `provider`, lowercased, so the existing unique key (user, kind, provider,
-- host_url) holds one row per account and an older server's upsert still
-- finds its conflict target. `is_default` marks the account machines get as
-- GH_TOKEN; `orgs` are the orgs its token could see when it was saved.
ALTER TABLE credentials ADD COLUMN is_default BOOLEAN NOT NULL DEFAULT false;
ALTER TABLE credentials ADD COLUMN orgs TEXT[] NOT NULL DEFAULT '{}';

UPDATE credentials SET provider = lower(coalesce(account, '')), is_default = true
WHERE kind = 'github';

CREATE UNIQUE INDEX credentials_one_default_github ON credentials (user_id)
WHERE kind = 'github' AND is_default;
