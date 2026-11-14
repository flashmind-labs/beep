### Added

- Node: when a coding agent sends a Beep problem report, the approval
  offers "Send and open issue". It also opens a public GitHub issue
  with your signed-in `gh` account. The issue carries the description,
  the report reference and the versions, but no logs. The agent gets
  the issue link back.
- Node: coding agents read Beep's status and logs first, and offer a
  problem report when Beep itself fails.
