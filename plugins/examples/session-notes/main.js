// Generated from main.ts with npm run plugins:build.

// main.ts
var NOTES_KEY = "notes";
function loadNotes() {
  const saved = beep.store.get(NOTES_KEY);
  if (typeof saved !== "object" || saved === null || Array.isArray(saved)) {
    return {};
  }
  return Object.fromEntries(
    Object.entries(saved).filter(
      (entry) => typeof entry[1] === "string"
    )
  );
}
var notes = beep.state(loadNotes());
var drafts = beep.state({});
function set(state, session, value) {
  state.set({ ...state.get(), [session]: value });
}
function keep(session, value) {
  set(notes, session, value.trim());
  beep.store.set(NOTES_KEY, notes.get());
}
function save(session, value) {
  keep(session, value);
  set(drafts, session, "");
}
beep.tool("get_note", ({ session }) => ({ note: notes.get()[session] ?? "" }));
beep.tool("set_note", ({ session, note }) => {
  keep(session, note);
  return { note: notes.get()[session] ?? "" };
});
beep.place({ slot: "session.row" }, (row) => {
  const note = notes.get()[row.session] ?? "";
  return note === "" ? null : beep.text(note, { tone: "accent" });
});
beep.place({ slot: "session.header" }, (header) => {
  const draft = drafts.get()[header.session] ?? "";
  return beep.row({ gap: 6 }, [
    beep.input({
      label: "Session note",
      placeholder: notes.get()[header.session] || "Add a note",
      value: draft,
      onChange: (value) => set(drafts, header.session, value),
      onSubmit: (value) => save(header.session, value)
    }),
    beep.button({ label: "Save note", onPress: () => save(header.session, draft) }, [
      beep.text("Save")
    ])
  ]);
});
