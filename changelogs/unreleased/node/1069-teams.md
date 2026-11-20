### Added

- Node: Serves a team's board to teammates over a new peer-to-peer
  link, with no server in between.
- Node: Agents get `team` and `team_ask` tools. `team` shows the
  team's work. `team_ask` messages a teammate's agent, at most 10
  messages an hour per session.
- Node: Starts an agent on each issue newly assigned to you on
  GitHub or Forgejo, once per issue. An optional limit caps how many
  of these agents run at once.
