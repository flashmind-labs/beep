// ---- Desktop: coding agents in the window (start form, one session, every session) ----
// Mirrors agentnew, agent and agents in apps/desktop/dist/app.js. Opened from the composer's Agent button,
// Cmd+Shift+A, a Running row on Home, or Running in the sidebar.
(() => {
  const st = document.createElement("style");
  st.textContent = `/* A call to the agent's own Beep tools (list_machines, run_on, sync_project, send_files, job_status, cancel, ask_user): what it did, the machine, the command or prompt, how many files, and how it stands. */
/* A thought: one line, "Thinking" with a sweep while it streams, "Thought for 12s" once done; a click opens the text. */
/* "Thinking" while it thinks: its letters light one after another, a calm loop (2.4s), each fading back behind the next. */
/* A thought while it streams shows open as plain text, in the thought's color, as it arrives (the app reads the session every 0.3s while a thought is open). At most 9 lines; it follows the newest, older lines scroll up and out under a fade. Done, it folds to "Thought for Ns" unless the reader opened or scrolled it. */
/* A turn's heading carries its folded work: "Working for 2m 27s · Ran 24 commands (5 failed), read 2 files", one line, one expander. */
/* The transcript as rows: kind icon, verb, target, detail, status; opened, the body under it. */
/* The session fills the window: the conversation, then the media pane to its right or under it. */
/* Split docks: the bottom dock sits under the conversation, the right one beside both. */
/* The Files tab: filter, tree, git marks, drop target; code and terminal views; mention chips. */
/* The Artifacts tab: what the agent published or wrote, newest first, then the picked one rendered. */
/* Too narrow for two columns: the pane goes under the conversation whatever its dock. */
@container (max-width: 700px) { }`;
  document.head.append(st);
})();
