### Added

- Node: agents can name the agents they start. Each new thread starts with a
  summary of the conversation so far.
- Node: when you write to a chat or one of its threads, the agent first hears
  what the other threads came to since its last turn. A thread hears what the
  main chat last said.
- Node: each new assistant conversation gets its own folder under
  `~/.beep/assistant/`, and each of its threads a subfolder named after it,
  for notes and findings. Deleting the conversation removes its folder.

### Changed

- Node: a thread no longer makes its idle parent chat run a turn when you
  wrote to the thread or stopped it. The parent hears of it at its next turn
  instead.
