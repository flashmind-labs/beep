### Fixed

- Node: flsh is installed and updated in `~/.local/bin`, where flsh's own
  installer puts it, instead of beep-node's own folder. A copy left in
  beep-node's folder moves there, or is removed when `~/.local/bin` already
  has flsh. Before, sessions kept running the node's older copy after flsh
  was updated with its own installer, so assistant chats on flsh could not
  reach Beep's tools and started no threads.
