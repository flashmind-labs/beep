// A project in a command center window of its own. The window to use, an
// open one or a new one, is picked in Rust (`center::open_project`); this
// is the part the page decides.

/// What a click needs to say, as a `MouseEvent` has it.
export interface ClickLike {
  button: number;
  metaKey: boolean;
}

/// The menu line that opens a project in its own window.
export const NEW_WINDOW = "Open in New Window";

/// Whether a click opens the project in a window of its own: ⌘ with the
/// main button, or the middle button.
export function opensNewWindow(e: ClickLike): boolean {
  return e.button === 1 || (e.button === 0 && e.metaKey);
}

/// The view a window starts on: its project when it was opened on one,
/// else what the bar or tray left waiting (null).
export function startView(project: string | null): { view: "projects"; arg: { project: string } } | null {
  return project ? { view: "projects", arg: { project } } : null;
}

/// The window's title: its project's name, Beep for the main window.
export function windowTitle(project: string | null): string {
  return project || "Beep";
}
