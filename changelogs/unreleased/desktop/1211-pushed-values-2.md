### Fixed

- Machine names, agent hosts and the session list now update as soon as a change arrives, without a second read that could show an older state.
- A branch's ahead and behind counts and a project's session list no longer step back to older values after an update.
- Renaming a project, removing a machine from it or changing its folder no longer reloads all of Home.

### Performance

- Several Codex sign-ins waiting at once are checked together, and the check no longer runs on every line a process prints.
