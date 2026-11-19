### Changed

- Desktop: Beep's bundled GitHub and Forgejo agent skills now live in Beep's
  own skills folder, `~/.config/beep/skills` (or
  `$XDG_CONFIG_HOME/beep/skills`; the config folder on Windows), instead of
  the folders Claude Code, Codex and Hermes use. Agents Beep starts learn
  about every skill in that folder, so skills you add there reach only
  Beep's agents.
