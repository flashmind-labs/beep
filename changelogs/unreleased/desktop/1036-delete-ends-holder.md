### Changed

- Desktop: deleting a session that is open in a terminal or IDE first ends the process holding it, asking it to exit and stopping it after 5 seconds. A session with no known process still refuses to delete.
