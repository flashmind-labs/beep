### Fixed
- A Git fetch you ask for now updates the ahead and behind counts even when another read finishes first.
- A machine that is slow to send its session list is read directly instead of showing old sessions.
- Messages and starts waiting to send are saved in the order they changed, so a quit or crash does not bring back a sent message.
- After a machine reconnects, its sessions show at once instead of waiting for a second read.

### Performance
- Project pages redraw their session lists only when that machine's sessions changed, at most once a second.
