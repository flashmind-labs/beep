---
name: beep_join_team
description: Use when the user wants to join a Beep team with a beepteam1 invite code, asks about team invites, or needs help joining or troubleshooting a team.
---

# Joining a Beep team

## Joining with a code

Use the `team_join` MCP tool when the user shares a `beepteam1...` invite code or asks to join a team:

- Pass `ticket` with the `beepteam1...` code.
- Pass `machine` (a name from `list_machines`) and `folder` (parent folder for cloning) only if the user specifies where the project should live.
- Beep asks the user to confirm before making changes, automatically imports a matching local checkout or clones the repository if it is not a project yet, proves push access on the forge, and joins the team.

Users can also join in the Beep desktop app from **Projects** -> **Join team**, **Search (`⌘K`)** -> **Join team**, or **Accept** on a contact's invite card under **Needs you**.

## Requirements

- Push access to the repository on GitHub or Forgejo.
- An online Beep desktop (desktops write mesh project and team state) and an online machine that has or can clone the checkout.
- A signed-in forge CLI on the checkout machine (`gh` for GitHub, `teacli` for Forgejo) and Git push credentials for the remote.
- A valid `beepteam1` code (codes expire after 24 hours).

## Privacy

Teammates see the user's cards on that project's board and can message their agents in that project. They never see the user's sessions, files, terminals, or other projects.

## Troubleshooting

- **Expired or invalid code**: ask a teammate to generate a new invite code from the project's **Team** tab.
- **Forge login error**: run `gh auth status` or `teacli auth config list` on the checkout machine, then guide the user to sign in (`gh auth login` or `teacli auth login`) or add the Forgejo host in Settings -> Credentials.
- **Clone or unknown host error**: run `list_machines` to pick an online machine with access to the forge, or verify the Forgejo host is configured, then retry `team_join` with `machine` (and `folder` if needed).
- **Removed or left before**: a current team member must click **Allow back** on the project's **Team** tab before the user can rejoin.
- **Never share or repeat invite codes** in public places (issues, PRs, commits).
