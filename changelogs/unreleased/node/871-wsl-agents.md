### Added

- Node: on a Windows machine, Codex, Gemini CLI, pi, flsh, Hermes and
  OpenCode installed in a WSL distribution now run as that machine's
  agents when Windows lacks them, as Claude Code already did. They need
  their ACP adapter or npx installed inside WSL.
- Node: sessions that coding agents ran inside WSL now show in Beep's
  session lists and transcripts, under their Windows folders.

### Fixed

- Node: file paths in a WSL session's tool calls and diffs now read as
  Windows paths, so they open.
