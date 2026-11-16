### Changed

- iOS: a new agent session opens at once when you send its first
  message from a project, Home, the assistant, Review with agent or
  Work on it. The app keeps the start on this device and starts the
  agent in the background. The session shows your message with what
  it is waiting for or doing under it.
- iOS: if the machine is offline, the start waits and runs when the
  machine comes online, also after the app restarts. A waiting start
  is listed with your sessions and can be cancelled.
- iOS: a start that fails shows the reason, with Retry and Cancel.

### Fixed

- iOS: a conversation just started no longer drops the message you
  sent and shows "Starting the agent" for a few seconds while its
  machine sets the model and thinking level.
