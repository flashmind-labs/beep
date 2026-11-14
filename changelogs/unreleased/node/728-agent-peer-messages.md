### Added

- Node: `agent list` takes a project name or `all` and shows every running
  agent on every machine, not only the ones you started. Agents in the same
  project can find each other whatever harness they run.
- Node: `agent send` messages any agent from that list. The receiver gets the
  message as a new turn and can reply with `agent send`. An agent takes at
  most five messages from other agents between two messages from you, so
  agents cannot keep each other talking.
