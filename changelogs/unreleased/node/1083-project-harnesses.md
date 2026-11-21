### Added

- beep-node: a session cannot start or resume with a harness that its project
  does not allow. This applies when you start it, when another agent starts it
  with start_agent or review, and when a session moves in. The error names the
  allowed harnesses.

### Changed

- beep-node: the harness "Can start" limit no longer applies to agents that
  a session starts. Limit harnesses per project instead.
