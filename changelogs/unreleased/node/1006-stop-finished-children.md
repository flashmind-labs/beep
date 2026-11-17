### Fixed

- Node: an agent that starts a helper agent is now told to stop the helper
  with `stop_agent` once it no longer needs it. Finished helpers used to stay
  open and listed as running until the agent that started them ended.
