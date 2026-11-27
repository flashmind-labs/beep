### Added
- Beep Node supports live in-turn steering for `pi` sessions through ACP `_session/steering`.

### Fixed
- Beep Node finishes stopping `pi` turns as soon as the model stream aborts instead of timing out on slow extension `agent_settled` hooks, and preserves queued steers if an unstoppable session is later resumed.
- Beep Node inherits the earlier session's title when starting a continued session across harnesses.
