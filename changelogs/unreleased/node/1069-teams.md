### Added

- Node: beep-node serves a team's board to teammates over a new peer-to-peer
  link, with no server in between.
- Node: agents get `team` and `team_ask` tools. `team` shows the
  team's work. `team_ask` messages a teammate's agent, at most 10
  messages an hour per session.
- Node: beep-node starts an agent on each issue newly assigned to you on
  GitHub or Forgejo, once per issue. An optional limit caps how many
  of these agents run at once.
- Node: everyone on a team must be able to push to the repository, when
  they join and after. A teammate is taken on only when GitHub or Forgejo
  confirms they can push (on Forgejo, where only admins see others'
  access, their push proof counts). Once a day beep-node checks you can
  still push, and leaves the team for you when you can't.
