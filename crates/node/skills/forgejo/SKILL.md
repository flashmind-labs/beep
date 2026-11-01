---
name: forgejo
description: Use when a task touches a project on Forgejo or Gitea, such as pull requests, issues, Actions runs, commit status or releases, including repositories that are also mirrored on GitHub.
---

# Forgejo and Gitea with teacli

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
| `beep.forgejo-host` | which teacli server | a name from `teacli auth config list` |

- A job set to `github` belongs to the `github` skill.
- A job with no key: if every remote is on Forgejo or Gitea, use it. If the
  project also has a GitHub remote, ask the user which host to use before you create anything there. To read
  (CI status, open PRs, issues), check every host and say which one each
  answer came from. Do not
  decide from `.github/` or `.forgejo/` folders, or from where issues happen
  to exist.

## Picking the server

The API refuses anonymous calls, so use `teacli`, not `curl`.

1. `teacli auth config list` shows the configured servers by name and URL.
2. With more than one, every command needs `--host <name>`. Use
   `beep.forgejo-host` when it is set. Otherwise pick the server whose URL
   matches the host and port of the project's Forgejo remote.
3. No server matches: tell the user to run `teacli auth login`.

Inside the checkout, `teacli` reads `<owner>/<repo>` from the remotes;
`teacli repo current --host <name>` shows what it found.

## Quick reference

| Need | Command |
|---|---|
| CI for a commit | `teacli api /repos/<owner>/<repo>/commits/<sha>/status --host <name>` |
| Runs on a branch | `teacli action list --branch main --host <name>` |
| Jobs and logs of a run | `teacli action jobs <run-id>`, then `teacli action logs <job-id>` |
| Open PRs | `teacli pr list` |
| One PR | `teacli pr get <number>` |
| Issues | `teacli issue list`, `teacli issue get <number>` |
| Releases | `teacli release list` |
| Anything else | `teacli schema --format json`, or `teacli api <path>` |

Add `--format json` when you need to parse the output.

## Common mistakes

- Matching runs to a commit through `teacli action list`: its commit fields
  can be empty. Ask for the commit's status instead.
- Leaving out `--host` with several servers configured: teacli exits 2.
- Creating, commenting on, merging or closing anything the user did not ask
  for. Reading is free; writing needs the user's request.
