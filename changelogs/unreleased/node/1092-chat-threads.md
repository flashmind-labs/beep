### Added

- Node: agents can name the agents they start. The assistant starts each
  thread of a chat with a summary of the conversation so far.
- Node: when you write to a session, its agent first hears what the agents
  it started came to since its last turn. An agent started by another hears
  what that one last said.
- Node: each new assistant conversation gets its own folder under
  `~/.beep/assistant/`, and each of its threads a subfolder named after it,
  for notes and findings. Deleting the conversation removes its folder.

### Changed

- Node: an agent that another agent started no longer makes its idle parent
  run a turn when you wrote to it or stopped it. The parent hears of it at
  its next turn instead.
