// The command center window's shared state: what Home and the projects
// last read, and the page cache kept between launches.
import { PageCache, freshness, type PageSection, type SectionStore } from "../project/pagecache.ts";
import { invoke } from "../ui/tauri.ts";
import { pane, type PaneTarget } from "../bar/pane.ts";
import type { HomeView, MacControl, SyncRow } from "./home-state.ts";
import type { FoundRow, OverviewCard, Placed, Project, ProjectGroup, ProjectMachineRow, SessionRow } from "../project/projects-state.ts";
import type { GitItemsView } from "./openitems-state.ts";
import type { ProjectAgent, ProjectView } from "../project/agent.ts";

export interface Started {
  session: SessionRow;
  why: string;
}

// ---------- Window state ----------

/// What a view's mount gets: a pane target, or the Projects page's pick.
export type ViewArg = PaneTarget | ProjectsArg | undefined;
export interface ProjectsArg { project: string | null }

/// A view's header: its title and line, buttons, and an element beside the title.
export interface ViewHead { title: string; sub?: string; tools?: Node[]; scope?: Node }

export interface CenterView {
  el?: HTMLElement;
  title?: string | (() => ViewHead);
  mount(el: HTMLElement, arg: ViewArg): void;
  unmount?(): void;
}

/// How a Needs you card or a project card was answered.
export interface Outcome { said?: string; error?: string; final?: boolean }

/// A chat or group in the sidebar, or a project pinned among them.
export type ChatItem = Awaited<ReturnType<typeof pane.chatRows>>[number];
export interface ProjectPin { kind: "project"; id: string; title: string; pinned: true; project: Placed<Project> }
export type SideChat = ChatItem | ProjectPin;

export const $ = <T extends HTMLElement = HTMLElement>(sel: string) => document.querySelector<T>(sel)!;


export const IC = {
  home: '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round"><path d="M3 11l9-7 9 7v9a1 1 0 01-1 1h-5v-6H9v6H4a1 1 0 01-1-1z"/></svg>',
  run: '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><circle cx="12" cy="12" r="9"/><path d="M10 8l6 4-6 4z" fill="currentColor"/></svg>',
  cal: '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><rect x="3" y="5" width="18" height="16" rx="2"/><path d="M3 10h18M8 3v4M16 3v4"/></svg>',
  chat: '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round"><path d="M4 5h16v11H9l-5 4z"/></svg>',
  mem: '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round"><path d="M6 3h12v18l-6-4-6 4z"/></svg>',
  find: '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><circle cx="11" cy="11" r="7"/><path d="M20 20l-4-4"/></svg>',
  loop: '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M17 2l4 4-4 4"/><path d="M3 11V9a3 3 0 013-3h15M7 22l-4-4 4-4"/><path d="M21 13v2a3 3 0 01-3 3H3"/></svg>',
  plus: '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M12 5v14M5 12h14"/></svg>',
  gear: '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.7 1.7 0 00.3 1.8l.1.1a2 2 0 11-2.8 2.8l-.1-.1a1.7 1.7 0 00-1.8-.3 1.7 1.7 0 00-1 1.5V21a2 2 0 11-4 0v-.1a1.7 1.7 0 00-1.1-1.5 1.7 1.7 0 00-1.8.3l-.1.1a2 2 0 11-2.8-2.8l.1-.1a1.7 1.7 0 00.3-1.8 1.7 1.7 0 00-1.5-1H3a2 2 0 110-4h.1a1.7 1.7 0 001.5-1.1 1.7 1.7 0 00-.3-1.8l-.1-.1a2 2 0 112.8-2.8l.1.1a1.7 1.7 0 001.8.3H9a1.7 1.7 0 001-1.5V3a2 2 0 114 0v.1a1.7 1.7 0 001 1.5 1.7 1.7 0 001.8-.3l.1-.1a2 2 0 112.8 2.8l-.1.1a1.7 1.7 0 00-.3 1.8V9a1.7 1.7 0 001.5 1H21a2 2 0 110 4h-.1a1.7 1.7 0 00-1.5 1z"/></svg>',
  x: '<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round"><path d="M6 6l12 12M18 6L6 18"/></svg>',
  pin: '<svg width="12" height="12" viewBox="0 0 24 24" fill="currentColor"><path d="M15 3l6 6-3 1-4 4 1 5-2 2-4-4-5 5-1-1 5-5-4-4 2-2 5 1 4-4z"/></svg>',
  group: '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round"><circle cx="9" cy="8" r="3.5"/><path d="M2.5 20a6.5 6.5 0 0113 0"/><circle cx="17" cy="9" r="2.5"/><path d="M16 14a5 5 0 016 5"/></svg>',
  doc: '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round"><path d="M6 2.5h8l4.5 4.5v14.5H6z"/><path d="M14 2.5V7h4.5"/></svg>',
  pr: '<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><circle cx="6" cy="5" r="2.2"/><circle cx="6" cy="19" r="2.2"/><circle cx="18" cy="19" r="2.2"/><path d="M6 7.2v9.6M18 16.8V10a3 3 0 00-3-3h-4M13 4.5L10.5 7 13 9.5"/></svg>',
  issue: '<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><circle cx="12" cy="12" r="8.5"/><circle cx="12" cy="12" r="1.6" fill="currentColor"/></svg>',
  rout: '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><circle cx="12" cy="12" r="9"/><path d="M12 7v5l3 2"/></svg>',
  term: '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><rect x="3" y="4" width="18" height="16" rx="2"/><path d="M7 9l3 3-3 3M13 15h4"/></svg>',
  folder: '<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round"><path d="M3 6.5A1.5 1.5 0 014.5 5H10l2 2.5h7.5A1.5 1.5 0 0121 9v9.5a1.5 1.5 0 01-1.5 1.5h-15A1.5 1.5 0 013 18.5z"/></svg>',
};

export const PROJECT_COLORS = ["#F07A6A", "#6FA8DC", "#B18CE0", "#E9B949", "#3B9B7A"];
export const OFFLINE_NOTE_MS = 60 * 60 * 1000;
/// Before Home first loads: how long "Connecting" shows before "Still
/// connecting", and how soon a failed try is made again.
export const CONNECT_STUCK_MS = 15000;
export const CONNECT_RETRY_MS = 3000;

/// `since` is when the window started waiting for its first Home.
export const state = {
  home: null as HomeView | null,
  mac: null as MacControl | null,
  error: null as string | null,
  outcomes: new Map<string, Outcome>(),
  since: Date.now(),
  picked: null as string | null,
  found: null as FoundRow[] | null,
  importing: null as string | null,
  foundError: null as { name: string; message: string } | null,
  sessions: new Map<string, SessionRow[]>(),
  where: new Map<string, ProjectMachineRow[] | null>(),
  agents: undefined as ProjectAgent[] | null | undefined,
  pview: new Map<string, { view: ProjectView | null; error: string | null }>(),
  sync: [] as SyncRow[],
  git: null as GitItemsView | null,
  gitAll: new Set<string>(),
  top: [] as string[],
  overview: undefined as OverviewCard[] | null | undefined,
  groups: undefined as ProjectGroup[] | null | undefined,
  renaming: null as { id: string; draft: string } | null,
};
/// What each project's page last read, section by section: drawn at once when a
/// project opens, then read again quietly.
export const pages = new PageCache(null);
/// The saved cache from `center_cached`, until this Mac's account is known.
let pagesFile: unknown;
export function keepPagesFile(file: unknown) { pagesFile = file; }
const PAGES_SAVE_MS = 5000;
let pagesTimer: ReturnType<typeof setTimeout> | undefined;

function keepPages() {
  clearTimeout(pagesTimer);
  pagesTimer = setTimeout(() => {
    if (!pages.takeChanged()) return;
    const file = pages.toFile();
    if (file) invoke("center_keep_pages", { pages: file }).catch(e => console.warn("keeping the project pages:", e));
  }, PAGES_SAVE_MS);
}

/// Keeps `data` as what `section` of `id` read now.
export function keep(section: PageSection, id: string, data: unknown) {
  pages.put(section, id, data);
  keepPages();
}

/// `section` of the cache for threads.ts and review.ts, saved on change.
export function keptSection<T>(section: PageSection): SectionStore<T> {
  const store = pages.section<T>(section);
  return { ...store, put: (id, data) => { store.put(id, data); keepPages(); } };
}

/// Takes this Mac's account once known: the saved cache comes in for it,
/// and another account's goes.
export function pagesAccount(mac: MacControl | null) {
  if (!mac?.id) return;
  if (pagesFile !== undefined) {
    pages.restore(pagesFile, mac.id);
    pagesFile = undefined;
  } else {
    pages.setAccount(mac.id);
  }
}

/// Fills in project `name`'s sections from the cache where the window has
/// not read them yet.
export function warm(name: string) {
  if (!state.pview.get(name)?.view) {
    const view = pages.get<ProjectView>("view", name);
    if (view) state.pview.set(name, { view, error: state.pview.get(name)?.error ?? null });
  }
  if (!state.where.get(name)) {
    const where = pages.get<ProjectMachineRow[]>("where", name);
    if (where) state.where.set(name, where);
  }
  if (!state.sessions.has(name)) {
    const sessions = pages.get<SessionRow[]>("sessions", name);
    if (sessions) state.sessions.set(name, sessions);
  }
}

/// A section header's note for `section` of `id`, `loading` while it reads.
export function pageFresh(section: PageSection, id: string, loading: boolean) {
  return freshness(pages.at(section, id), loading, Date.now());
}

/// The views the window can show, by name.
export const views = new Map<string, CenterView & { el: HTMLElement }>();

/// A sidebar group.
export interface SidebarGroup<T> {
  order: number;
  title?: string;
  load(): Promise<T[] | null>;
  row(item: T): HTMLElement;
  onPick?(item: T): void;
  onTitle?(): void;
  action?(): HTMLElement;
  empty?: string | (() => string | HTMLElement);
  foot?(): HTMLElement[];
}

export interface GroupSlot<T> extends SidebarGroup<T> {
  title: string;
  items: T[] | null;
  loading: boolean;
  again: boolean;
}

export const groups = new Map<string, GroupSlot<unknown>>();
