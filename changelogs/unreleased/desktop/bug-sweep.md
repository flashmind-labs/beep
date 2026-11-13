### Fixed

- Desktop: a node connection that goes silent is noticed and opened again
  instead of waiting forever.
- Desktop: messages kept for an agent on another machine are sent after the
  app restarts.
- Desktop: on Linux, logs are written under the XDG state folder instead of
  ~/Library/Logs/Beep.
- Desktop: one terminal that cannot be cleaned up no longer empties the whole
  terminal list.
- Desktop: a refused pause, resolve or start-over of file sync shows its
  error.
- Desktop: Send in a review works again after a failed send, and "Try again"
  in a problem report works more than once.
- Desktop: Preview in the Files menu opens the preview instead of the source.
- Desktop: a failed command shows the same exit code in every row.
- Desktop: Home shows Gemini CLI, pi and flsh by name.
- Desktop: a session in a nested project is counted under the same project
  everywhere.
- Desktop: one machine on an older version no longer stops agent modes from
  loading for other machines.
- Desktop: terminal text with both strikethrough and overline shows both.
- Desktop: saving two agent settings at once keeps both, and a newly added
  token with push access is used at once.
- Desktop: a refused clone from GitLab, Bitbucket or another Git host is
  reported as not found or no access.
