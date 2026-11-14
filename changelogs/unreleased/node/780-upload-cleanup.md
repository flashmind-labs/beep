### Changed

- beep-node: A session's temporary attachments are deleted one day after
  the session ends, unless the session runs again.
- beep-node: Any attachment, including files dropped on a new agent's
  prompt into a project's `.beep/uploads` folder, is deleted after three
  days without being read or written.
