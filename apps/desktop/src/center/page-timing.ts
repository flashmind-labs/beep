// Names never contain a project's name, a session id, or another page argument.
export function pageName(name: string, arg: unknown): string {
  if (name !== "pane") return name;
  if (arg && typeof arg === "object") {
    if ("agent" in arg || "draft" in arg) return "agent";
    if ("agentnew" in arg) return "agentnew";
    if ("chat" in arg) return "chat";
    if ("group" in arg) return "group";
  }
  if (typeof arg === "string" && ["agent", "agentnew", "agents", "chat", "chats", "group", "ginfo", "gnew", "invite", "memories", "routines", "rnew", "grants", "skills", "apps", "appperms", "mcp", "machines", "settings", "model", "link", "login", "starting", "joincode", "joining", "invite-phone", "revoked", "research"].includes(arg)) return arg;
  return "pane";
}

/// Times the first painted frame after the current page's mount finishes.
export class PageTiming {
  private visit = 0;
  private readonly now: () => number;
  private readonly frame: (callback: () => void) => void;
  private readonly visible: () => boolean;
  private readonly record: (page: string, ms: number) => void;
  constructor(
    now: () => number, frame: (callback: () => void) => void,
    visible: () => boolean, record: (page: string, ms: number) => void,
  ) { this.now = now; this.frame = frame; this.visible = visible; this.record = record; }

  start(page: string): (mountedPage?: string) => void {
    const at = this.now(), visit = ++this.visit, visible = this.visible();
    return mountedPage => this.frame(() => this.frame(() => {
      if (visible && this.visible() && visit === this.visit) this.record(mountedPage ?? page, this.now() - at);
    }));
  }
}
