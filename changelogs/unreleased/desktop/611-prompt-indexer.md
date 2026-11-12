### Performance

- Desktop: the palette's prompt search reads only a session's new
  transcript entries, once the session has been quiet for a minute, instead
  of rereading whole transcripts every 30 seconds. A slow link keeps the
  pages it already read, and a session that fails to load waits longer
  before the next try.
