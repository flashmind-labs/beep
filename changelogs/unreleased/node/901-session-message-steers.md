### Fixed

- Node: a message one agent session sends another with `send_to_agent`
  reaches a working session at its next step, as a steer, instead of
  waiting for the turn to end. Pressing Stop on that session no longer
  deletes these messages. They go out as the next prompt.
