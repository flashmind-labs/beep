### Fixed

- Team cards keep updating when the machine acting for the team runs an older Beep node (macOS, Windows, Linux).
- A terminal's or task's final lines and its exit show without reopening it, including on older nodes (macOS, Windows, Linux).
- A team that could not be read when its project opened is read again when the projects change (macOS, Windows, Linux).
- Opening an agent that was just started waits for it even when other sessions change meanwhile (macOS, Windows, Linux).

### Performance

- The machine list no longer waits for the slow reads of each machine's agents and project pages (macOS, Windows, Linux).
- The sync and move notices no longer redraw views while every window is hidden (macOS, Windows, Linux).
- Team cards are followed only while a project's page shows them (macOS, Windows, Linux).
