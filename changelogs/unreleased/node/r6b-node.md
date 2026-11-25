### Changed
- The desktop on this machine hears of a team invite the moment it arrives or is dropped, instead of asking again on a timer.
- A terminal's state reaches a window that follows that one terminal alone, instead of the whole terminal list.
- A machine that acts for no team no longer wakes every 10 seconds to elect a team's gateway: it elects when the mesh, a roster or a host's reach changes.
- Agent spend figures update when an agent's log changes, instead of on a 60 second timer.
- Asking a node to sync a project returns as soon as the project settles, with no half-second wait loop.
- Syncthing coming back, or changing a folder, is noticed from its event stream instead of a 5 minute recheck.
