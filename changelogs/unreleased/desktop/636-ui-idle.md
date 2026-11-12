### Performance

- Desktop: working agents, running commands, CI runs and research show a still
  dot instead of a turning spinner, so an idle window with agents at work no
  longer redraws every frame. The thinking sweep stops while the window is
  hidden, and a terminal cursor blinks only while its window has focus.
- Desktop: the built-in node runs git less often while agents work: a git
  command no longer makes it rewatch every folder and read them all again,
  file sync reads the checked-out commit once per change, and a folder's file
  list waits longer between reads while it stays the same.
