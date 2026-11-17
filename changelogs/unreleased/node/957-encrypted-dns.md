### Fixed

- Node: on Wi-Fi networks that rewrite DNS answers, such as some hotel
  content filters, the node could not reach any machine. Names now resolve
  through encrypted DNS first, then the network's DNS if that fails.
