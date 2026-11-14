### Fixed

- Node: after you stop a turn and send a message, the message goes once the
  turn has stopped. Before, it went into the stopping turn, so the agent kept
  working and the session stayed on Stopping.
