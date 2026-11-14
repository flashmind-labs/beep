/// <reference path="../../sdk/beep.d.ts" />

// A short note on each session's row, set from the open session's header.
// Notes live in memory, so a reload clears them.

type Notes = Readonly<Record<string, string>>;

const notes = beep.state<Notes>({});
const drafts = beep.state<Notes>({});

function set(state: State<Notes>, session: string, value: string): void {
  state.set({ ...state.get(), [session]: value });
}

function save(session: string, value: string): void {
  set(notes, session, value.trim());
  set(drafts, session, "");
}

// Nothing is drawn for a session without a note, so its row does not change.
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
      onSubmit: (value) => save(header.session, value),
    }),
    beep.button({ label: "Save note", onPress: () => save(header.session, draft) }, [
      beep.text("Save"),
    ]),
  ]);
});
