### Changed

- Node: a Claude Code turn that was cut off reads as an "Interrupted" note instead of a message from you.
- Node: an update from Beep waits up to 30 minutes, instead of 2, for running agent turns to finish before it restarts the node.

### Fixed

- Node: a subagent's Beep tool calls read by their title, such as "Run on <machine>: <command>", instead of the raw tool name.
