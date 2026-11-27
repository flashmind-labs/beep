import { projectWindowProps } from "../../parts/project/projectops.ts";
import { placeWindowParts } from "../../parts/center/window-root.tsx";
import { windowHost } from "../../parts/center/window-shell.tsx";
import { NoticesView } from "./notices-view.tsx";
// The bell at the right end of the main window header, beside the usage
// pill, and the notifications pane it opens: what the mesh told this Mac (a
// coding agent finished or asks, sync conflicts, a failed move, a machine that
// went offline).
import type { PaneTarget } from "../../shell/bar/pane.ts";
import type { ProjectsArg } from "../../stores/center/state.ts";
import { reason } from "../../ui/format.ts";
import { invoke, listen } from "../../ui/tauri.ts";
import { commandKey, WINDOWS } from "../../ui/platform.ts";
import { type Held, type Note, conflictProject, find, fromNote, markRead, opens, skillName, unreadIds, without } from "../../stores/center/notices-state.ts";
import { go, openScreen } from "../../parts/center/nav.ts";
import { sessions } from "../../stores/agents/sessions.ts";
import { chatOf } from "../../stores/agents/threads-state.ts";

/// Opens the pane. Set by `mountNotices`.
let show: () => void = () => {};

/// A notification's click: the pane opened.
export function showNotices() {
  show();
}

/// Draws the header's bell and keeps it current.
export function mountNotices() {
  const host = windowHost("bell");
  let notes: Note[] = [];
  let open = false;
  let opened: string | null = null;
  let returnFocus: HTMLElement | null = null;
  /// Asks being answered, and why an answer failed, by id.
  const answering = new Set<string>();
  const failed = new Map<string, string>();
  /// Skill conflicts with a copy being kept, by id: the copy's hash and the note as it was.
  const keeping = new Map<string, { hash: string; was: string }>();
  /// What a note says, read or not.
  const told = (i: Note | null) => JSON.stringify(i && { ...i, read: true });

  const setNotes = (next: Note[]) => {
    notes = next.map(fromNote).sort((a, b) => b.at - a.at);
    // A conflict being kept waits until the node tells it again.
    for (const [id, k] of keeping) if (told(find(notes, id)) !== k.was) keeping.delete(id);
    // A skill conflict that is gone shows the list.
    if (opened && !find(notes, opened)) opened = null;
    draw();
  };

  const focusList = (id?: string | null) => {
    const rows = [...host.querySelectorAll<HTMLElement>(".nt-row")];
    (rows.find(r => r.dataset.id === id) ?? rows[0] ?? host.querySelector<HTMLElement>(".nt-pop"))?.focus();
  };

  const backToList = () => {
    const id = opened;
    opened = null;
    draw();
    focusList(id);
  };

  const toggle = (to: boolean, keyboard = false) => {
    if (to && !open) returnFocus = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    open = to;
    if (!to) opened = null;
    draw();
    // Opening the pane reads what it shows; the rows keep their dots until it closes.
    if (to) read(unreadIds(notes), false);

    if (keyboard) {
      if (to) focusList();
      else (returnFocus?.isConnected ? returnFocus : host.querySelector<HTMLElement>(".nt-bell"))?.focus();
    }
  };

  const draw = () => {
    const active = document.activeElement;
    const inPop = active instanceof HTMLElement && host.contains(active) && !!active.closest(".nt-pop");
    const focused = inPop ? active.closest<HTMLElement>(".nt-row") : null;
    const index = focused ? [...host.querySelectorAll(".nt-row")].indexOf(focused) : -1;
    const action = focused ? [...focused.querySelectorAll("button")].indexOf(active as HTMLButtonElement) : -1;
    const shown = opened ? find(notes, opened) : null;
    placeWindowParts({ notices: <NoticesView
      windowProps={projectWindowProps} chatOf={threadChat} items={notes} open={open} shown={shown} answering={answering} failed={failed} keeping={keeping} toggle={() => toggle(!open)} back={backToList}
      dismiss={dismiss} openItem={openItem} answer={answer} keep={keepCopy}
    /> });
    if (focused && !focused.isConnected && open) {
      const rows = [...host.querySelectorAll<HTMLElement>(".nt-row")];
      const row = rows.find(r => r.dataset.id === focused.dataset.id) ?? rows[Math.min(index, rows.length - 1)];
      const control = row?.querySelectorAll("button")[action];
      (control && !control.disabled ? control : row ?? host.querySelector<HTMLElement>(".nt-pop button") ?? host.querySelector<HTMLElement>(".nt-pop"))?.focus();
    } else if (inPop && open && !shown && active === host.querySelector(".nt-pop")) focusList();
  };

  /// Marks `list` read, here and where it is kept.
  const read = (list: string[], redraw = true) => {
    const fresh = list.filter(id => find(notes, id)?.read === false);
    if (!fresh.length) return;
    notes = markRead(notes, fresh);
    if (redraw) draw();
    invoke("notes_read", { ids: fresh }).catch(e => console.warn("reading notifications:", e));
  };

  const dismiss = (list: string[]) => {
    if (!list.length) return;
    notes = without(notes, list);
    if (opened && list.includes(opened)) opened = null;
    draw();
    invoke("notes_dismiss", { ids: list }).catch(e => {
      console.warn("dismissing notifications:", e);
      void loadNotes();
    });
  };

  /// A row's click: a skill conflict opens in the pane, the rest open what they are about.
  const openItem = (i: Note) => {
    const to = opens(i);
    read([i.id], false);

    if (to === "skill") {
      opened = i.id;
      draw();
      return;
    }

    if (!to) return;
    if (i.kind === "run") dismiss([i.id]);
    toggle(false);
    if (to === "link") invoke("open_url", { url: (i.target as { link: string }).link }).catch(e => console.warn("opening a link:", e));
    else if (to === "project") go("projects", (i.target as { projects: ProjectsArg }).projects);
    else if (to === "session") go("pane", i.target as PaneTarget);
    else if (to === "conflict") openConflict(conflictProject(i));
    else openScreen({ screen: "settings", section: "machines" });
  };

  /// A sync conflict opens its project's Machines section, or the projects when the note names none.
  const openConflict = (project: string | null) => go("projects", { project, tile: project ? "machines" : null });

  const answer = (i: Note, allow: boolean) => {
    answering.add(i.id);
    failed.delete(i.id);
    draw();
    invoke("notes_answer", { id: i.id, allow })
      .catch(e => failed.set(i.id, String(e)))
      .finally(() => { answering.delete(i.id); draw(); });
  };

  /// Asks the machine holding `held` to keep it; the row waits until the conflict is told again.
  const keepCopy = (i: Note, held: Held) => {
    keeping.set(i.id, { hash: held.hash, was: told(i) });
    failed.delete(i.id);
    draw();
    invoke("skill_keep", { machine: held.machine, name: skillName(i), hash: held.hash }).catch(e => {
      keeping.delete(i.id);
      failed.set(i.id, reason(e));
      draw();
    });
  };

  show = () => {
    const already = open;
    open = true;
    opened = null;
    draw();
    if (!already) read(unreadIds(notes), false);
  };

  const loadNotes = async () => {
    try {
      setNotes(await invoke<Note[]>("notes_list"));
    } catch (e) {
      console.warn("reading notifications:", e);
    }
  };

  document.addEventListener("click", e => { if (open && !e.composedPath().includes(host)) toggle(false); });
  document.addEventListener("keydown", e => {
    if (!e.defaultPrevented && commandKey(e) && !(WINDOWS ? e.metaKey : e.ctrlKey) && !e.shiftKey && !e.altKey && e.key.toLowerCase() === "b" &&
        !(e.target instanceof Element && e.target.closest("[contenteditable]:not([contenteditable=false]), .tm-term"))) {
      e.preventDefault();
      toggle(!open, true);
      return;
    }

    if (!open || e.key !== "Escape") return;
    e.preventDefault();
    // Esc goes back to the list first, then closes the pane.
    if (opened) backToList(); else toggle(false, true);
  });
  listen<Note[]>("notes", e => setNotes(e.payload));
  draw();
  void loadNotes();
}

/// The title of the chat whose thread `note` is about, null for a chat's own note or any other.
function threadChat(note: Note): string | null {
  const target = note.target && typeof note.target === "object" && "agent" in note.target ? note.target.agent : null;
  const row = target && sessions.find(one => one.machine_id === target.machine_id && one.id === target.id);
  const chat = row ? chatOf(sessions, row) : null;
  return chat && chat !== row ? chat.title || "Assistant" : null;
}
