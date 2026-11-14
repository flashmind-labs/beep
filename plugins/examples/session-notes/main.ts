/// <reference path="../../sdk/beep.d.ts" />

// A short note on each session's row, set from the open session's header.
// Notes are kept in the plugin's store, so they survive a restart.

type Notes = Readonly<Record<string, string>>;

const NOTES_KEY = "notes";

/** The saved notes, or none when the store holds something else. */
function loadNotes(): Notes {
  const saved: unknown = beep.store.get(NOTES_KEY);
  if (typeof saved !== "object" || saved === null || Array.isArray(saved)) {
    return {};
  }

  return Object.fromEntries(
    Object.entries(saved).filter(
      (entry): entry is [string, string] => typeof entry[1] === "string",
    ),
  );
}

const notes = beep.state<Notes>(loadNotes());
const drafts = beep.state<Notes>({});

function set(state: State<Notes>, session: string, value: string): void {
  state.set({ ...state.get(), [session]: value });
}

function save(session: string, value: string): void {
  set(notes, session, value.trim());
  beep.store.set(NOTES_KEY, notes.get());
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
