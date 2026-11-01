---
name: github
description: Use when a task touches a project on GitHub, such as pull requests, issues, CI runs and checks, releases or forks, including repositories that are also hosted on Forgejo or Gitea.
---

# GitHub with gh

## Which host does what

A project can live on GitHub, on Forgejo, or on both, and the user picks
which host handles each job. Read the choice before anything else:

```bash
git config --get-regexp '^beep\.'
```

| Key | Job | Values |
|---|---|---|
| `beep.prs` | pull requests | `github`, `forgejo` |
| `beep.issues` | issues | `github`, `forgejo` |
| `beep.ci` | CI and builds | `github`, `forgejo` |
| `beep.releases` | releases | `github`, `forgejo` |

- A job set to `forgejo` belongs to the `forgejo` skill.
- A job with no key: if every remote is on github.com, use GitHub. If the
  project also has a Forgejo or Gitea remote, ask the user which host to
  use. Do not decide from `.github/` or `.forgejo/` folders, or from where
  issues happen to exist.

Remote names say nothing. Find the GitHub remote by its URL. In a fork, push
to the remote you own and open the pull request against the parent
(`gh repo view --json parent`), following its CONTRIBUTING file.

## Quick reference

| Need | Command |
|---|---|
| CI for a commit | `gh run list --commit <sha>` |
| Checks on a PR | `gh pr checks <number>` |
| Logs of a failed run | `gh run view <id> --log-failed` |
| Open PRs | `gh pr list` |
| One PR | `gh pr view <number> --comments` |
| Issues | `gh issue list`, `gh issue view <number>` |
| Releases | `gh release list` |
| Anything else | `gh api repos/<owner>/<repo>/...` |

Add `-R <owner>/<repo>` when the checkout has more than one GitHub remote,
and `--json <fields>` when you need to parse the output.

## Common mistakes

- Creating, commenting on, merging or closing anything the user did not ask
  for. Reading is free; writing needs the user's request.
- Treating `origin` as the user's repository. In forks it is often the
  parent.
